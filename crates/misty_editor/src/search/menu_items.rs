//! Menu commands the palette can run: the menu bar's entries as a flat, searchable list.
//!
//! Titles read like commands ("Create Cube", "Lighting"); `path` is where the item lives in the
//! menu bar. Entries that the menu bar handles without an [`Action`] get their own [`MenuRun`].

use misty_engine::scene::{LightType, PrimitiveMesh, registry};

use misty_engine::preset;

use crate::actions::{Action, AssetKind, Create, SceneOp};
use crate::layout::{BuiltinLayout, Tab};
use crate::layouts::LayoutRef;
use crate::state::EditorState;

/// What running a menu command does.
#[derive(Clone, Debug)]
pub enum MenuRun {
    Action(Action),
    /// Edit ▸ Preferences.
    Preferences,
    /// Assets ▸ Refresh.
    RefreshAssets,
    /// GameObject ▸ Light ▸ Reflection Probe / Generate Reflection Probes.
    CreateProbe,
    GenerateProbes,
    /// Help ▸ About.
    About,
    /// Help ▸ Tutorials.
    Tutorials,
}

#[derive(Clone, Debug)]
pub struct MenuCommand {
    /// Searchable name ("Create Cube").
    pub title: String,
    /// Menu bar path ("GameObject/3D Object/Cube").
    pub path: String,
    pub shortcut: &'static str,
    pub run: MenuRun,
}

/// Collects commands with a fluent helper.
struct List(Vec<MenuCommand>);

impl List {
    fn add(&mut self, title: impl Into<String>, path: impl Into<String>, run: MenuRun) {
        self.0.push(MenuCommand {
            title: title.into(),
            path: path.into(),
            shortcut: "",
            run,
        });
    }

    fn action(&mut self, title: &str, path: &str, shortcut: &'static str, a: Action) {
        self.0.push(MenuCommand {
            title: title.into(),
            path: path.into(),
            shortcut,
            run: MenuRun::Action(a),
        });
    }

    fn create(&mut self, what: Create, menu: &str) {
        let name = what.default_name();
        let action = Action::Create { what, parent: None };
        let path = format!("GameObject/{menu}{name}");
        self.0.push(MenuCommand {
            title: format!("Create {name}"),
            path,
            shortcut: "",
            run: MenuRun::Action(action),
        });
    }
}

/// Every menu command for the current editor state (components, layouts, recent scenes).
pub fn all(s: &EditorState) -> Vec<MenuCommand> {
    let mut l = List(Vec::new());
    file(&mut l, s);
    edit(&mut l);
    assets(&mut l);
    gameobject(&mut l);
    for info in registry::components().iter().filter(|c| c.user_visible()) {
        let path = format!("Component/{}/{}", info.category, info.name);
        l.action(
            &format!("Add {}", info.name),
            &path,
            "",
            Action::AddComponent(info.name),
        );
    }
    presets(&mut l, s);
    window(&mut l, s);
    l.add("Tutorials", "Help/Tutorials...", MenuRun::Tutorials);
    l.add(
        "About Misty Engine",
        "Help/About Misty Engine",
        MenuRun::About,
    );
    l.0
}

fn file(l: &mut List, s: &EditorState) {
    l.action("New Scene", "File/New Scene", "Ctrl+N", Action::NewScene);
    l.action("Open Scene", "File/Open Scene", "Ctrl+O", Action::OpenScene);
    for rel in &s.files.settings.recent_scenes {
        let name = rel.rsplit('/').next().unwrap_or(rel);
        let path = format!("File/Open Recent Scene/{rel}");
        l.action(
            &format!("Open {name}"),
            &path,
            "",
            Action::OpenScenePath(rel.clone()),
        );
    }
    for rel in &s.files.settings.recent_scenes {
        let name = rel.rsplit('/').next().unwrap_or(rel);
        let path = format!("File/Open Recent Scene/{rel} (Additive)");
        l.action(
            &format!("Open {name} Additive"),
            &path,
            "",
            Action::OpenSceneAdditive(rel.clone()),
        );
    }
    l.action("Save", "File/Save", "Ctrl+S", Action::SaveScene);
    l.action(
        "Save As",
        "File/Save As...",
        "Ctrl+Shift+S",
        Action::SaveSceneAs,
    );
    open_scenes(l, s);
    let build = Action::OpenBuildSettings;
    l.action(
        "Build Settings",
        "File/Build Settings...",
        "Ctrl+Shift+B",
        build,
    );
    l.action(
        "Build And Run",
        "File/Build And Run",
        "Ctrl+B",
        Action::BuildAndRun,
    );
    l.action(
        "New Project",
        "File/New Project...",
        "",
        Action::OpenHub { new_project: true },
    );
    l.action(
        "Open Project",
        "File/Open Project...",
        "",
        Action::OpenHub { new_project: false },
    );
    l.action(
        "Open Misty Hub",
        "File/Open Misty Hub",
        "",
        Action::OpenHub { new_project: false },
    );
    l.action("Exit", "File/Exit", "", Action::Quit);
}

/// The Hierarchy scene header menu of every open scene ("Set Active Scene Lighting").
fn open_scenes(l: &mut List, s: &EditorState) {
    let scene = &s.scene;
    let several = scene.scenes().len() > 1;
    for entry in scene.scenes() {
        let id = entry.id;
        let name = scene.scene_name(id);
        let header = format!("Hierarchy/{name}");
        let mut op = |title: &str, item: &str, op: SceneOp| {
            let path = format!("{header}/{item}");
            l.action(&format!("{title} {name}"), &path, "", Action::Scene(id, op));
        };
        if !entry.loaded {
            op("Load Scene", "Load Scene", SceneOp::Load);
            continue;
        }
        if scene.active_scene() != id {
            op("Set Active Scene", "Set Active Scene", SceneOp::SetActive);
        }
        if several {
            op("Save Scene", "Save Scene", SceneOp::Save);
            if scene.loaded_scenes().len() > 1 {
                op("Unload Scene", "Unload Scene", SceneOp::Unload);
            }
            op("Remove Scene", "Remove Scene", SceneOp::Remove);
            op(
                "Close All Scenes Except",
                "Close Others",
                SceneOp::CloseOthers,
            );
        }
    }
}

/// Every component preset, applied to the selection ("Apply Preset Warm Street Lamp").
fn presets(l: &mut List, s: &EditorState) {
    let entities = s.selection.list().to_vec();
    if entities.is_empty() {
        return;
    }
    for (rel, p) in &s.assets.presets {
        let asset = [preset::MATERIAL, preset::TEXTURE_IMPORTER].contains(&p.target.as_str());
        if asset {
            continue;
        }
        let name = crate::presets::preset_name(rel);
        let path = format!("Component/Presets/{}/{name}", p.target);
        let action = Action::ApplyPreset {
            entities: entities.clone(),
            preset: rel.clone(),
        };
        l.action(&format!("Apply Preset {name}"), &path, "", action);
    }
}

fn edit(l: &mut List) {
    let rows: [(&str, &str, Action); 22] = [
        ("Undo", "Ctrl+Z", Action::Undo),
        ("Redo", "Ctrl+Y", Action::Redo),
        ("Select All", "Ctrl+A", Action::SelectAll),
        ("Deselect All", "Shift+D", Action::DeselectAll),
        ("Cut", "Ctrl+X", Action::CutObjects),
        ("Copy", "Ctrl+C", Action::CopyObjects),
        (
            "Paste",
            "Ctrl+V",
            Action::PasteObjects {
                text: None,
                as_child: false,
            },
        ),
        ("Duplicate", "Ctrl+D", Action::DuplicateSelected),
        ("Rename", "F2", Action::RenameSelected),
        ("Delete", "Del", Action::DeleteSelected),
        ("Frame Selected", "F", Action::FrameSelected),
        ("Lock View to Selected", "Shift+F", Action::ToggleLockView),
        ("Align With View", "Ctrl+Shift+F", Action::AlignWithView),
        ("Grid and Snap", "", Action::OpenGridAndSnap),
        ("Align To Grid", "", Action::AlignToGrid),
        ("Play", "Ctrl+P", Action::TogglePlay),
        ("Pause", "Ctrl+Shift+P", Action::TogglePause),
        ("Step", "Ctrl+Alt+P", Action::Step),
        ("Project Settings", "", Action::OpenProjectSettings),
        ("Move To View", "Ctrl+Alt+F", Action::MoveToView),
        ("Align View to Selected", "", Action::AlignViewToSelected),
        ("Toggle Active State", "Alt+Shift+A", Action::ToggleActive),
    ];
    for (title, shortcut, action) in rows {
        // The last three live in the GameObject menu.
        let menu = if matches!(
            action,
            Action::MoveToView | Action::AlignViewToSelected | Action::ToggleActive
        ) {
            "GameObject"
        } else {
            "Edit"
        };
        l.action(title, &format!("{menu}/{title}"), shortcut, action);
    }
    let history = Tab::UndoHistory;
    l.action(
        history.title(),
        "Edit/Undo History",
        history.shortcut(),
        Action::FocusTab(history),
    );
    l.add("Preferences", "Edit/Preferences...", MenuRun::Preferences);
}

fn assets(l: &mut List) {
    let kinds = AssetKind::ALL.into_iter().chain([
        AssetKind::SurfaceShader,
        AssetKind::UnlitShader,
        AssetKind::MaterialFromShader,
    ]);
    for kind in kinds {
        let name = kind.label();
        let path = format!("Assets/Create/{name}");
        l.action(
            &format!("Create {name}"),
            &path,
            "",
            Action::CreateAsset(kind),
        );
    }
    for k in crate::tile_palette::TileCreate::ALL {
        let name = k.label();
        l.action(
            &format!("Create {name}"),
            &format!("Assets/Create/2D/{name}"),
            "",
            Action::CreateAsset(AssetKind::Tile2D(k)),
        );
    }
    l.add("Refresh", "Assets/Refresh", MenuRun::RefreshAssets);
}

fn gameobject(l: &mut List) {
    l.create(Create::Empty, "");
    if let Some(empty) = l.0.last_mut() {
        empty.title = "Create Empty".into();
        empty.path = "GameObject/Create Empty".into();
        empty.shortcut = "Ctrl+Shift+N";
    }
    l.create(Create::Folder, "");
    if let Some(folder) = l.0.last_mut() {
        folder.title = "Create Folder".into();
        folder.path = "GameObject/Create Folder".into();
    }
    for p in PrimitiveMesh::ALL {
        l.create(Create::Primitive(p), "3D Object/");
    }
    l.create(Create::Terrain, "3D Object/");
    l.create(Create::Sprite(None), "2D Object/");
    for shape in misty_engine::sprites::builtin::SHAPES {
        l.create(Create::Sprite(Some(shape)), "2D Object/Sprites/");
    }
    l.create(Create::ParticleSystem, "Effects/");
    for kind in LightType::ALL {
        l.create(Create::Light(kind), "Light/");
    }
    let probe = "GameObject/Light/Reflection Probe";
    l.add("Create Reflection Probe", probe, MenuRun::CreateProbe);
    let generate = "GameObject/Light/Generate Reflection Probes";
    l.add(
        "Generate Reflection Probes",
        generate,
        MenuRun::GenerateProbes,
    );
    l.create(Create::AudioSource, "Audio/");
    l.create(Create::FogVolume, "Rendering/");
    for kind in misty_engine::water::WaterKind::ALL {
        l.create(Create::Water(kind), "Water/");
    }
    for el in crate::ui_create::UiElement::ALL {
        l.create(Create::Ui(el), "UI/");
    }
    l.create(Create::Camera, "");
    let parent = Action::CreateEmptyParent;
    l.action(
        "Create Empty Parent",
        "GameObject/Create Empty Parent",
        "Ctrl+Shift+G",
        parent,
    );
    l.action(
        "Clear Parent",
        "GameObject/Clear Parent",
        "",
        Action::ClearParent,
    );
}

fn window(l: &mut List, s: &EditorState) {
    // Undo History lives in the Edit menu (listed by `edit`).
    for tab in Tab::every().filter(|t| *t != Tab::UndoHistory) {
        let menu = match tab {
            Tab::Animation | Tab::Animator => "Animation",
            Tab::AudioMixer => "Audio",
            Tab::Lighting => "Rendering",
            Tab::Navigation => "AI",
            Tab::WorldBuilder => "Terrain",
            Tab::Profiler => "Analysis",
            Tab::Timeline => "Sequencing",
            Tab::TilePalette => "2D",
            _ => "General",
        };
        let path = format!("Window/{menu}/{}", tab.title());
        l.action(tab.title(), &path, tab.shortcut(), Action::FocusTab(tab));
    }
    for b in BuiltinLayout::ALL {
        let path = format!("Window/Layouts/{}", b.name());
        let action = Action::LoadLayout(LayoutRef::Builtin(b));
        l.action(&format!("{} Layout", b.name()), &path, "", action);
    }
    for name in s.layouts.user.keys() {
        let path = format!("Window/Layouts/{name}");
        let action = Action::LoadLayout(LayoutRef::User(name.clone()));
        l.action(&format!("{name} Layout"), &path, "", action);
    }
    l.action(
        "Reset All Layouts",
        "Window/Layouts/Reset All Layouts",
        "",
        Action::ResetAllLayouts,
    );
}

/// Run a command (Enter on a menu result).
pub fn run(s: &mut EditorState, run: &MenuRun) {
    match run {
        MenuRun::Action(a) => s.push(a.clone()),
        MenuRun::Preferences => s.code_editor.ui.prefs_open = true,
        MenuRun::RefreshAssets => {
            s.refresh_assets();
            s.console.info("Asset database refreshed.");
        }
        MenuRun::CreateProbe => crate::probe_tools::create_probe(s, None),
        MenuRun::GenerateProbes => crate::probe_tools::generate(s),
        MenuRun::Tutorials => s.tutorials.window_open = true,
        MenuRun::About => s
            .console
            .info(concat!("Misty Engine ", env!("CARGO_PKG_VERSION"))),
    }
}
