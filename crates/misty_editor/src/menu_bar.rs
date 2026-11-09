//! Top menu bar: File, Edit, Assets, GameObject, Component, Tools, Window, Help.

use egui::Ui;

use misty_engine::hecs::Entity;
use misty_engine::scene::registry;
use misty_engine::scene::{LightType, PrimitiveMesh};

use crate::actions::{Action, Create};
use crate::i18n::{self, tr};
use crate::layout::{BuiltinLayout, Tab};
use crate::layouts::LayoutRef;
use crate::state::EditorState;
use crate::tutorials::targets;

/// Menu entry with a right-aligned shortcut hint; pushes `action` when clicked. `label` is the
/// English text (the tutorial target id); it is shown in the editor's language.
fn item(ui: &mut Ui, state: &mut EditorState, label: &str, shortcut: &str, action: Action) {
    item_shown(ui, state, label, tr(label), shortcut, action);
}

/// [`item`] showing `shown` (already in the editor's language) for the target `label`.
fn item_shown(
    ui: &mut Ui,
    state: &mut EditorState,
    label: &str,
    shown: String,
    shortcut: &str,
    action: Action,
) {
    let resp = ui.add(entry_button_shown(shown, shortcut));
    targets::header(ui, label, resp.rect);
    if resp.clicked() {
        state.push(action);
        ui.close();
    }
}

/// A menu entry's button: `label` in the editor's language and its shortcut hint (on the far
/// side, so on the left in a right-to-left UI).
pub fn entry_button(label: &str, shortcut: &str) -> egui::Button<'static> {
    entry_button_shown(tr(label), shortcut)
}

/// [`entry_button`] showing `text` (already in the editor's language).
fn entry_button_shown(text: String, shortcut: &str) -> egui::Button<'static> {
    if shortcut.is_empty() {
        egui::Button::new(text)
    } else if i18n::is_rtl() {
        let hint = egui::RichText::new(shortcut.to_owned()).weak();
        egui::Button::new((hint, egui::Atom::grow(), text))
    } else {
        egui::Button::new(text).shortcut_text(shortcut.to_owned())
    }
}

/// Menu contents: entries right-aligned in a right-to-left UI.
fn menu_body<R>(ui: &mut Ui, f: impl FnOnce(&mut Ui) -> R) -> R {
    if i18n::is_rtl() {
        ui.with_layout(egui::Layout::top_down_justified(egui::Align::Max), f)
            .inner
    } else {
        f(ui)
    }
}

/// Top menu button; it and its entries are tutorial targets (`menu.<Menu>/<Entry>`, English).
fn top(ui: &mut Ui, label: &str, f: impl FnOnce(&mut Ui)) -> egui::Rect {
    let id = format!("menu.{label}");
    let r = ui.menu_button(tr(label), |ui| {
        let _scope = targets::scope(&id);
        menu_body(ui, f)
    });
    targets::mark(ui, &id, r.response.rect);
    crate::capture::open_menu_from_env(ui, label, &r.response);
    r.response.rect
}

/// Submenu that is a tutorial target too (`…/<Submenu>/<Entry>`, English). In a right-to-left
/// UI its arrow is on the left.
pub fn sub(ui: &mut Ui, label: &str, f: impl FnOnce(&mut Ui)) {
    let content = |ui: &mut Ui| {
        let _scope = targets::scope(label);
        menu_body(ui, f)
    };
    let rect = if i18n::is_rtl() {
        let button = egui::Button::new(("\u{23f4}", egui::Atom::grow(), tr(label)));
        egui::containers::menu::SubMenuButton::from_button(button)
            .ui(ui, content)
            .0
            .rect
    } else {
        ui.menu_button(tr(label), content).response.rect
    };
    targets::header(ui, label, rect);
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

/// The menus; returns the far edge of the last one (the title bar centres its title past it):
/// its right edge, or its left edge in a right-to-left UI, where the menus run from the right
/// (File rightmost).
pub fn show_menus(ui: &mut Ui, state: &mut EditorState) -> f32 {
    let rtl = i18n::is_rtl();
    egui::MenuBar::new()
        .ui(ui, |ui| {
            if rtl {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    all_menus(ui, state).left()
                })
                .inner
            } else {
                all_menus(ui, state).right()
            }
        })
        .inner
}

/// Every top menu in order; returns the last one's rectangle.
fn all_menus(ui: &mut Ui, state: &mut EditorState) -> egui::Rect {
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
}

fn file_menu(ui: &mut Ui, s: &mut EditorState) {
    item(ui, s, "New Scene", "Ctrl+N", Action::NewScene);
    item(ui, s, "Open Scene", "Ctrl+O", Action::OpenScene);
    sub(ui, "Open Recent Scene", |ui| {
        let recent = s.files.settings.recent_scenes.clone();
        if recent.is_empty() {
            ui.weak(tr!("(none)"));
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
        "Build Profiles...",
        "Ctrl+Shift+B",
        Action::OpenBuildSettings,
    );
    item(ui, s, "Build And Run", "Ctrl+B", Action::BuildAndRun);
    ui.separator();
    item(ui, s, "Exit", "", Action::Quit);
}

fn edit_menu(ui: &mut Ui, s: &mut EditorState) {
    // "Undo Move" (the target keeps the English label).
    let undo = s.undo.undo_label().map(|l| {
        let shown = tr!("Undo {action}", action = tr(l));
        (format!("Undo {l}"), shown)
    });
    let redo = s.undo.redo_label().map(|l| {
        let shown = tr!("Redo {action}", action = tr(l));
        (format!("Redo {l}"), shown)
    });
    ui.add_enabled_ui(undo.is_some(), |ui| {
        let (label, shown) = undo.unwrap_or_else(|| ("Undo".into(), tr!("Undo")));
        item_shown(ui, s, &label, shown, "Ctrl+Z", Action::Undo)
    });
    ui.add_enabled_ui(redo.is_some(), |ui| {
        let (label, shown) = redo.unwrap_or_else(|| ("Redo".into(), tr!("Redo")));
        item_shown(ui, s, &label, shown, "Ctrl+Y", Action::Redo)
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
    let prefs = ui.add(entry_button("Preferences...", ""));
    if prefs.clicked() {
        s.code_editor.ui.prefs_open = true;
        ui.close();
    }
    todo(ui, s, "Shortcuts...", "");
}

fn assets_menu(ui: &mut Ui, s: &mut EditorState) {
    sub(ui, "Create", |ui| {
        crate::panels::project::create_menu(ui, s)
    });
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
        if ui.button(tr!("Rename")).clicked() {
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
        if ui.button(tr!("Delete")).clicked() {
            s.project.confirm_delete = Some(path);
            ui.close();
        }
    });
    ui.separator();
    todo(ui, s, "Import New Asset...", "");
    if ui.add(entry_button("Refresh", "Ctrl+R")).clicked() {
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
            ui.weak(tr!("Terrain + World Builder from a template"));
            for t in misty_engine::world_gen::templates::Template::ALL {
                let b = ui.button(t.label()).on_hover_text(t.description());
                targets::header(ui, t.label(), b.rect);
                if b.clicked() {
                    crate::panels::world_builder::templates::create_world(s, t, parent, true);
                    ui.close();
                }
            }
        });
        if ui.button(tr!("Mesh Builder Shape…")).clicked() {
            crate::mesh_builder::open_new_shape(s);
            ui.close();
        }
        let ragdoll = ui.button(tr!("Ragdoll…")).on_hover_text(tr!(
            "Colliders, rigidbodies and joints on a character's bones"
        ));
        targets::header(ui, "Ragdoll…", ragdoll.rect);
        if ragdoll.clicked() {
            crate::ragdoll_wizard::open(s, parent);
            ui.close();
        }
    });
    // Player ▸ HeadBobVertical ▸ HeadBobHorizontal ▸ Camera with the controller scripts.
    let fps = ui.button(crate::fps_player::NAME).on_hover_text(tr!(
        "A Character Controller with mouse look, walk / run / jump / crouch and head bob"
    ));
    targets::header(ui, crate::fps_player::NAME, fps.rect);
    if fps.clicked() {
        crate::fps_player::create(s, parent);
        ui.close();
    }
    sub(ui, "2D Object", |ui| {
        item(ui, s, "Sprite", "", create(Create::Sprite(None)));
        sub(ui, "Sprites", |ui| {
            for shape in misty_engine::sprites::builtin::SHAPES {
                item(ui, s, shape, "", create(Create::Sprite(Some(shape))));
            }
        });
        sub(ui, "Tilemap", |ui| {
            for layout in misty_engine::tilemap::GridLayout::ALL {
                let label = match layout {
                    misty_engine::tilemap::GridLayout::Rectangle => tr!("Rectangular"),
                    misty_engine::tilemap::GridLayout::Isometric => tr!("Isometric"),
                };
                if ui.button(label).clicked() {
                    crate::tile_palette::create_tilemap(s, parent, layout);
                    ui.close();
                }
            }
        });
    });
    sub(ui, "Effects", |ui| {
        item(ui, s, "Particle System", "", create(Create::ParticleSystem));
        item(ui, s, "Visual Effect", "", create(Create::VisualEffect));
    });
    sub(ui, "Light", |ui| {
        for l in LightType::ALL {
            let what = Create::Light(l);
            item(ui, s, what.default_name(), "", create(what));
        }
        ui.separator();
        for kind in misty_engine::light2d::Light2DType::ALL {
            let label = tr!("{kind} Light 2D", kind = tr(kind.label()));
            if ui.button(label).clicked() {
                crate::tile_palette::create_light_2d(s, parent, kind);
                ui.close();
            }
        }
        ui.separator();
        if ui.button(tr!("Reflection Probe")).clicked() {
            crate::probe_tools::create_probe(s, parent);
            ui.close();
        }
        if ui.button(tr!("Generate Reflection Probes")).clicked() {
            crate::probe_tools::generate(s);
            ui.close();
        }
        if ui.button(tr!("Light Probe Group")).clicked() {
            crate::light_probe_tools::create_group(s, parent);
            ui.close();
        }
    });
    sub(ui, "Audio", |ui| {
        item(ui, s, "Audio Source", "", create(Create::AudioSource));
    });
    sub(ui, "Rendering", |ui| {
        for what in [Create::FogVolume, Create::DecalProjector] {
            item(ui, s, what.default_name(), "", create(what));
        }
    });
    sub(ui, "Water", |ui| {
        for kind in misty_engine::water::WaterKind::ALL {
            item(ui, s, kind.label(), "", create(Create::Water(kind)));
        }
    });
    sub(ui, "UI", |ui| {
        for el in crate::ui_create::UiElement::ALL {
            item(ui, s, el.name(), "", create(Create::Ui(el)));
            if el.ends_group() {
                ui.separator();
            }
        }
    });
    sub(ui, "Camera", |ui| {
        item(ui, s, "Camera", "", create(Create::Camera));
        ui.separator();
        crate::vcam_tools::menu(ui, s, parent);
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

/// The Component menu's submenus (component categories).
pub const COMPONENT_CATEGORIES: [&str; 11] = [
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
];

fn component_menu(ui: &mut Ui, s: &mut EditorState) {
    item(ui, s, "Add...", "Ctrl+Shift+A", Action::OpenAddComponent);
    ui.separator();
    let has_selection = !s.selection.is_empty();
    for cat in COMPONENT_CATEGORIES {
        sub(ui, cat, |ui| {
            let entries: Vec<_> = registry::components()
                .iter()
                .filter(|c| c.category == cat)
                .collect();
            if entries.is_empty() {
                ui.weak(tr!("(coming soon)"));
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
    sub(ui, "Layouts", |ui| layouts_menu(ui, s));
    ui.separator();
    sub(ui, "General", |ui| {
        for tab in Tab::ALL.into_iter().filter(|t| *t != Tab::Animation) {
            item(ui, s, tab.title(), tab.shortcut(), Action::FocusTab(tab));
        }
        let tab = Tab::TestRunner;
        item(ui, s, tab.title(), tab.shortcut(), Action::FocusTab(tab));
        ui.separator();
        if ui.button(tr!("Save Data")).clicked() {
            crate::saveloc::save_data::open(s);
            ui.close();
        }
        if ui.button(tr!("Localization Tables")).clicked() {
            crate::saveloc::tables::open_window(s);
            ui.close();
        }
    });
    sub(ui, "Animation", |ui| {
        for tab in [Tab::Animation, Tab::Animator] {
            item(ui, s, tab.title(), tab.shortcut(), Action::FocusTab(tab));
        }
    });
    sub(ui, "Sequencing", |ui| {
        let tab = Tab::Timeline;
        item(ui, s, tab.title(), tab.shortcut(), Action::FocusTab(tab));
    });
    sub(ui, "Audio", |ui| {
        let tab = Tab::AudioMixer;
        item(ui, s, tab.title(), tab.shortcut(), Action::FocusTab(tab));
    });
    sub(ui, "Rendering", |ui| {
        for tab in [
            Tab::Lighting,
            Tab::ShaderGraph,
            Tab::EffectGraph,
            Tab::OcclusionCulling,
        ] {
            item(ui, s, tab.title(), tab.shortcut(), Action::FocusTab(tab));
        }
    });
    sub(ui, "Visual Scripting", |ui| {
        let tab = Tab::ScriptGraph;
        item(ui, s, tab.title(), tab.shortcut(), Action::FocusTab(tab));
    });
    sub(ui, "AI", |ui| {
        let tab = Tab::Navigation;
        item(ui, s, tab.title(), tab.shortcut(), Action::FocusTab(tab));
    });
    sub(ui, "2D", |ui| {
        let tab = Tab::TilePalette;
        item(ui, s, tab.title(), tab.shortcut(), Action::FocusTab(tab));
    });
    sub(ui, "Terrain", |ui| {
        let tab = Tab::WorldBuilder;
        item(ui, s, tab.title(), tab.shortcut(), Action::FocusTab(tab));
    });
    sub(ui, "World", |ui| {
        let tab = Tab::WorldPartition;
        item(ui, s, tab.title(), tab.shortcut(), Action::FocusTab(tab));
    });
    sub(ui, "Analysis", |ui| {
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
    let vcs = ui.button(tr!("Version Control…"));
    targets::header(ui, "Version Control…", vcs.rect);
    if vcs.clicked() {
        s.vcs.window.open = true;
        let root = s.project.root.clone();
        s.vcs.refresh_now(&root);
        ui.close();
    }
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
        if ui.selectable_label(name == current, tr(name)).clicked() {
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
    if ui.button(tr!("Save Layout...")).clicked() {
        s.layouts.save_prompt = Some(String::new());
        ui.close();
    }
    ui.add_enabled_ui(!names.is_empty(), |ui| {
        sub(ui, "Delete Layout", |ui| {
            for name in &names {
                item(ui, s, name, "", Action::DeleteLayout(name.clone()));
            }
        });
    });
    item(ui, s, "Reset All Layouts", "", Action::ResetAllLayouts);
}

fn help_menu(ui: &mut Ui, s: &mut EditorState) {
    let tutorials = ui.button(tr!("Tutorials..."));
    targets::header(ui, "Tutorials...", tutorials.rect);
    if tutorials.clicked() {
        s.tutorials.window_open = true;
        ui.close();
    }
    ui.separator();
    if ui.button(tr!("About Misty Engine")).clicked() {
        s.about_open = true;
        ui.close();
    }
    if ui.button(tr!("Check for Updates…")).clicked() {
        s.updater.check_now();
        ui.close();
    }
    todo(ui, s, "Manual", "");
    let reference = ui.button(tr!("Scripting Reference")).on_hover_text(tr!(
        "Write the Lua API reference to Library/ApiDocs and open it"
    ));
    targets::header(ui, "Scripting Reference", reference.rect);
    if reference.clicked() {
        crate::api_docs::open_reference(s);
        ui.close();
    }
    crate::live_reload::dev_build::menu(ui, s);
}
