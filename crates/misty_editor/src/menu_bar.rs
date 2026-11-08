//! Top menu bar: File, Edit, Assets, GameObject, Component, Tools, Window, Help.

use egui::Ui;

use misty_engine::hecs::Entity;
use misty_engine::scene::registry;
use misty_engine::scene::{LightType, PrimitiveMesh};

use crate::actions::{Action, Create};
use crate::layout::{BuiltinLayout, Tab};
use crate::layouts::LayoutRef;
use crate::state::EditorState;
use crate::tutorials::targets;

/// Menu entry with a right-aligned shortcut hint; pushes `action` when clicked.
fn item(ui: &mut Ui, state: &mut EditorState, label: &str, shortcut: &str, action: Action) {
    let mut button = egui::Button::new(label);
    if !shortcut.is_empty() {
        button = button.shortcut_text(shortcut);
    }
    let resp = ui.add(button);
    targets::header(ui, label, resp.rect);
    if resp.clicked() {
        state.push(action);
        ui.close();
    }
}

/// Top menu button; it and its entries are tutorial targets (`menu.<Menu>/<Entry>`).
fn top(ui: &mut Ui, label: &str, f: impl FnOnce(&mut Ui)) -> egui::Rect {
    let id = format!("menu.{label}");
    let r = ui.menu_button(label, |ui| {
        let _scope = targets::scope(&id);
        f(ui)
    });
    targets::mark(ui, &id, r.response.rect);
    r.response.rect
}

/// Submenu that is a tutorial target too (`…/<Submenu>/<Entry>`).
fn sub(ui: &mut Ui, label: &str, f: impl FnOnce(&mut Ui)) {
    let r = ui.menu_button(label, |ui| {
        let _scope = targets::scope(label);
        f(ui)
    });
    targets::header(ui, label, r.response.rect);
}

/// Entry that only logs "not implemented yet" for now.
fn todo(ui: &mut Ui, state: &mut EditorState, label: &'static str, shortcut: &str) {
    item(ui, state, label, shortcut, Action::NotImplemented(label));
}

/// The menus on their own (tests).
#[cfg(test)]
pub fn show(ui: &mut Ui, state: &mut EditorState) {
    show_menus(ui, state);
}

/// The menus; returns the right edge of the last one (the title bar centres its title past it).
pub fn show_menus(ui: &mut Ui, state: &mut EditorState) -> f32 {
    egui::MenuBar::new()
        .ui(ui, |ui| {
            // Editor script menu items join the built-in menus at the end, and their own
            // menus ("Tools") sit between Component and Window.
            let scripts = crate::editor_scripts::menus::menu_ui;
            top(ui, "File", |ui| {
                file_menu(ui, state);
                scripts(ui, state, "File");
            });
            top(ui, "Edit", |ui| {
                edit_menu(ui, state);
                scripts(ui, state, "Edit");
            });
            top(ui, "Assets", |ui| {
                assets_menu(ui, state);
                scripts(ui, state, "Assets");
            });
            top(ui, "GameObject", |ui| {
                gameobject_menu(ui, state);
                scripts(ui, state, "GameObject");
            });
            top(ui, "Component", |ui| {
                component_menu(ui, state);
                scripts(ui, state, "Component");
            });
            top(ui, "Tools", |ui| {
                tools_menu(ui, state);
                scripts(ui, state, "Tools");
            });
            for name in crate::editor_scripts::menus::extra_tops(state) {
                top(ui, &name, |ui| scripts(ui, state, &name));
            }
            top(ui, "Window", |ui| {
                window_menu(ui, state);
                scripts(ui, state, "Window");
            });
            top(ui, "Help", |ui| {
                help_menu(ui, state);
                scripts(ui, state, "Help");
            })
            .right()
        })
        .inner
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
    item(ui, s, "Reload Scene", "Ctrl+Shift+R", Action::ReloadScene);
    ui.separator();
    item(
        ui,
        s,
        "New Project...",
        "",
        Action::OpenHub { new_project: true },
    );
    item(
        ui,
        s,
        "Open Project...",
        "",
        Action::OpenHub { new_project: false },
    );
    item(
        ui,
        s,
        "Open Misty Hub",
        "",
        Action::OpenHub { new_project: false },
    );
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
    let history = Tab::UndoHistory;
    item(
        ui,
        s,
        history.title(),
        history.shortcut(),
        Action::FocusTab(history),
    );
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
    if ui.button("Preferences...").clicked() {
        s.code_editor.ui.prefs_open = true;
        ui.close();
    }
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
    item(ui, s, "Create Folder", "", create(Create::Folder));
    sub(ui, "3D Object", |ui| {
        for p in PrimitiveMesh::ALL {
            item(ui, s, p.name(), "", create(Create::Primitive(p)));
        }
        ui.separator();
        item(ui, s, "Terrain", "", create(Create::Terrain));
        sub(ui, "World", |ui| {
            ui.weak("Terrain + World Builder from a template");
            for t in misty_engine::world_gen::templates::Template::ALL {
                let b = ui.button(t.label()).on_hover_text(t.description());
                targets::header(ui, t.label(), b.rect);
                if b.clicked() {
                    crate::panels::world_builder::templates::create_world(s, t, parent, true);
                    ui.close();
                }
            }
        });
        if ui.button("Mesh Builder Shape…").clicked() {
            crate::mesh_builder::open_new_shape(s);
            ui.close();
        }
    });
    // Player ▸ HeadBobVertical ▸ HeadBobHorizontal ▸ Camera with the controller scripts.
    let fps = ui.button(crate::fps_player::NAME).on_hover_text(
        "A Character Controller with mouse look, walk / run / jump / crouch and head bob",
    );
    targets::header(ui, crate::fps_player::NAME, fps.rect);
    if fps.clicked() {
        crate::fps_player::create(s, parent);
        ui.close();
    }
    ui.menu_button("2D Object", |ui| {
        item(ui, s, "Sprite", "", create(Create::Sprite(None)));
        ui.menu_button("Sprites", |ui| {
            for shape in misty_engine::sprites::builtin::SHAPES {
                item(ui, s, shape, "", create(Create::Sprite(Some(shape))));
            }
        });
        ui.menu_button("Tilemap", |ui| {
            for layout in misty_engine::tilemap::GridLayout::ALL {
                let label = match layout {
                    misty_engine::tilemap::GridLayout::Rectangle => "Rectangular",
                    misty_engine::tilemap::GridLayout::Isometric => "Isometric",
                };
                if ui.button(label).clicked() {
                    crate::tile_palette::create_tilemap(s, parent, layout);
                    ui.close();
                }
            }
        });
    });
    ui.menu_button("Effects", |ui| {
        item(ui, s, "Particle System", "", create(Create::ParticleSystem));
        item(ui, s, "Visual Effect", "", create(Create::VisualEffect));
    });
    ui.menu_button("Light", |ui| {
        for l in LightType::ALL {
            let what = Create::Light(l);
            item(ui, s, what.default_name(), "", create(what));
        }
        ui.separator();
        for kind in misty_engine::light2d::Light2DType::ALL {
            if ui.button(format!("{} Light 2D", kind.label())).clicked() {
                crate::tile_palette::create_light_2d(s, parent, kind);
                ui.close();
            }
        }
        ui.separator();
        if ui.button("Reflection Probe").clicked() {
            crate::probe_tools::create_probe(s, parent);
            ui.close();
        }
        if ui.button("Generate Reflection Probes").clicked() {
            crate::probe_tools::generate(s);
            ui.close();
        }
        if ui.button("Light Probe Group").clicked() {
            crate::light_probe_tools::create_group(s, parent);
            ui.close();
        }
    });
    ui.menu_button("Audio", |ui| {
        item(ui, s, "Audio Source", "", create(Create::AudioSource));
    });
    sub(ui, "Rendering", |ui| {
        for what in [Create::FogVolume, Create::DecalProjector] {
            item(ui, s, what.default_name(), "", create(what));
        }
    });
    ui.menu_button("Water", |ui| {
        for kind in misty_engine::water::WaterKind::ALL {
            item(ui, s, kind.label(), "", create(Create::Water(kind)));
        }
    });
    ui.menu_button("UI", |ui| {
        for el in crate::ui_create::UiElement::ALL {
            item(ui, s, el.name(), "", create(Create::Ui(el)));
            if el.ends_group() {
                ui.separator();
            }
        }
    });
    ui.menu_button("Camera", |ui| {
        item(ui, s, "Camera", "", create(Create::Camera));
        ui.separator();
        for (label, free_look) in [("Virtual Camera", false), ("Free Look Camera", true)] {
            if ui.button(label).clicked() {
                crate::vcam_tools::create(s, parent, free_look);
                ui.close();
            }
        }
    });
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
    virtual_geometry_items(ui, s);
}

fn tools_menu(ui: &mut Ui, s: &mut EditorState) {
    use crate::vgeo_convert::{Command, Scope};
    sub(ui, "Rendering", |ui| {
        for scope in [Scope::Scene, Scope::Project] {
            let action = Action::VirtualGeometry(Command::ConvertDialog(scope));
            item(ui, s, &format!("{}...", scope.title()), "", action);
        }
    });
}

/// GameObject ▸ Enable / Disable Virtual Geometry: only the ones that apply to the selection.
pub fn virtual_geometry_items(ui: &mut Ui, s: &mut EditorState) {
    use crate::vgeo_convert::Command;
    let (off, on) = crate::vgeo_convert::selection_state(s);
    if !off && !on {
        return;
    }
    ui.separator();
    if off {
        let action = Action::VirtualGeometry(Command::EnableSelected);
        item(ui, s, "Enable Virtual Geometry", "", action);
    }
    if on {
        let action = Action::VirtualGeometry(Command::DisableSelected);
        item(ui, s, "Disable Virtual Geometry", "", action);
    }
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
        "Netcode",
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
        let tab = Tab::TestRunner;
        item(ui, s, tab.title(), tab.shortcut(), Action::FocusTab(tab));
        ui.separator();
        if ui.button("Save Data").clicked() {
            crate::saveloc::save_data::open(s);
            ui.close();
        }
        if ui.button("Localization Tables").clicked() {
            crate::saveloc::tables::open_window(s);
            ui.close();
        }
    });
    ui.menu_button("Animation", |ui| {
        for tab in [Tab::Animation, Tab::Animator] {
            item(ui, s, tab.title(), tab.shortcut(), Action::FocusTab(tab));
        }
    });
    ui.menu_button("Sequencing", |ui| {
        let tab = Tab::Timeline;
        item(ui, s, tab.title(), tab.shortcut(), Action::FocusTab(tab));
    });
    ui.menu_button("Audio", |ui| {
        let tab = Tab::AudioMixer;
        item(ui, s, tab.title(), tab.shortcut(), Action::FocusTab(tab));
    });
    ui.menu_button("Rendering", |ui| {
        for tab in [
            Tab::Lighting,
            Tab::ShaderGraph,
            Tab::EffectGraph,
            Tab::OcclusionCulling,
        ] {
            item(ui, s, tab.title(), tab.shortcut(), Action::FocusTab(tab));
        }
    });
    ui.menu_button("AI", |ui| {
        let tab = Tab::Navigation;
        item(ui, s, tab.title(), tab.shortcut(), Action::FocusTab(tab));
    });
    ui.menu_button("2D", |ui| {
        let tab = Tab::TilePalette;
        item(ui, s, tab.title(), tab.shortcut(), Action::FocusTab(tab));
    });
    sub(ui, "Terrain", |ui| {
        let tab = Tab::WorldBuilder;
        item(ui, s, tab.title(), tab.shortcut(), Action::FocusTab(tab));
    });
    ui.menu_button("Analysis", |ui| {
        for tab in [Tab::Profiler, Tab::FrameDebugger, Tab::MemoryProfiler] {
            item(ui, s, tab.title(), tab.shortcut(), Action::FocusTab(tab));
        }
    });
    sub(ui, "Asset Management", |ui| {
        let tab = Tab::AssetGroups;
        item(ui, s, tab.title(), tab.shortcut(), Action::FocusTab(tab));
    });
    sub(ui, "Input", |ui| {
        let tab = Tab::InputActions;
        item(ui, s, tab.title(), tab.shortcut(), Action::FocusTab(tab));
    });
    sub(ui, "Multiplayer", |ui| {
        let open = Action::OpenMultiplayerPlayMode;
        item(ui, s, "Multiplayer Play Mode", "", open);
    });
    ui.separator();
    item(
        ui,
        s,
        "Package Manager…",
        "",
        Action::FocusTab(Tab::PackageManager),
    );
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
    let tutorials = ui.button("Tutorials...");
    targets::header(ui, "Tutorials...", tutorials.rect);
    if tutorials.clicked() {
        s.tutorials.window_open = true;
        ui.close();
    }
    ui.separator();
    if ui.button("About Misty Engine").clicked() {
        s.about_open = true;
        ui.close();
    }
    if ui.button("Check for Updates…").clicked() {
        s.updater.check_now();
        ui.close();
    }
    todo(ui, s, "Manual", "");
    todo(ui, s, "Scripting Reference", "");
    crate::live_reload::dev_build::menu(ui, s);
}
