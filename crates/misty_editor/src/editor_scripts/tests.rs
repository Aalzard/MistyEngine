//! Editor scripting: the Lua editor API, tool windows and custom Inspectors (kittest), menu
//! items, undo, hot reload, gizmos, handles, error reporting and the SampleProject scripts.

use std::path::{Path, PathBuf};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use misty_engine::hecs::Entity;
use misty_engine::scene::{ScriptRef, ScriptValue, Scripts};

use super::*;
use crate::actions::Action;
use crate::console::LogLevel;
use crate::project::ProjectBrowser;

fn temp_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "misty_editor_scripts_{name}_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets")).unwrap();
    root
}

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

/// A project with `files`, its editor state and the editor scripts loaded.
fn project(name: &str, files: &[(&str, &str)]) -> EditorState {
    let root = temp_root(name);
    for (rel, text) in files {
        write(&root, rel, text);
    }
    let mut s = EditorState::new(ProjectBrowser::open(root.clone()));
    s.editor_scripts.use_prefs_file(root.join("prefs.json"));
    reload(&mut s);
    s
}

fn cleanup(s: EditorState) {
    let _ = std::fs::remove_dir_all(&s.project.root);
}

fn errors(s: &EditorState) -> Vec<String> {
    s.console
        .entries
        .iter()
        .filter(|e| e.level == LogLevel::Error)
        .map(|e| e.message.clone())
        .collect()
}

/// Run a Lua chunk in the editor state (as a script would, inside a call).
fn lua(s: &mut EditorState, code: &str) -> Result<(), String> {
    call(s, CALL_BUDGET, |lua| {
        lua.load(code)
            .set_name("test")
            .exec()
            .map_err(|e| describe(&e))
    })
    .expect("editor scripts are loaded")
}

/// Evaluate a Lua expression to a string (`tostring`).
fn eval(s: &mut EditorState, expr: &str) -> String {
    call(s, CALL_BUDGET, |lua| {
        lua.load(format!("return tostring({expr})"))
            .eval::<String>()
            .unwrap_or_else(|e| format!("error: {}", describe(&e)))
    })
    .expect("editor scripts are loaded")
}

fn names(s: &EditorState) -> Vec<String> {
    s.scene
        .walk()
        .into_iter()
        .map(|(e, _)| s.scene.name(e))
        .collect()
}

fn attach(s: &mut EditorState, e: Entity, path: &str) {
    s.scene.add(
        e,
        Scripts {
            list: vec![ScriptRef {
                path: path.into(),
                ..Default::default()
            }],
        },
    );
}

const TOOLS: &str = r#"
MenuItem("Tools/Make Marker", function()
    Undo.Record(nil, "Make Marker")
    EditorScene.Create("Marker")
end)
MenuItem("Tools/Nested/Deep Item", function() end, { priority = 5 })
MenuItem("Tools/Needs Selection", function()
    EditorScene.Create("Selected Tool Ran")
end, {
    validate = function() return Selection.count > 0 end,
    shortcut = "Ctrl+Alt+M",
})
MenuItem("GameObject/Script Item", function() end)
"#;

#[test]
fn menu_items_register_validate_and_run() {
    let mut s = project("menus", &[("Assets/Editor/Tools.lua", TOOLS)]);
    assert!(errors(&s).is_empty(), "{:?}", errors(&s));
    let paths: Vec<&str> = s
        .editor_scripts
        .menus
        .iter()
        .map(|m| m.path.as_str())
        .collect();
    assert_eq!(
        paths,
        [
            "Tools/Make Marker",
            "Tools/Nested/Deep Item",
            "Tools/Needs Selection",
            "GameObject/Script Item"
        ]
    );
    // "Tools" is a built-in menu (Rendering ▸ Convert … to Misty Mesh): the script items
    // join it.
    assert!(menus::extra_tops(&s).is_empty());
    // Running an item changes the scene; validate keeps the other one off without a selection.
    assert!(menus::run(&mut s, "Tools/Make Marker"));
    assert!(names(&s).contains(&"Marker".to_owned()));
    s.selection.clear();
    assert!(!menus::valid(&mut s, "Tools/Needs Selection"));
    menus::run(&mut s, "Tools/Needs Selection");
    assert!(!names(&s).contains(&"Selected Tool Ran".to_owned()));
    let cam = s.scene.find_by_name("Main Camera").unwrap();
    s.selection.set(cam);
    assert!(menus::valid(&mut s, "Tools/Needs Selection"));
    menus::run(&mut s, "Tools/Needs Selection");
    assert!(names(&s).contains(&"Selected Tool Ran".to_owned()));
    // Shortcuts parse; bad ones are refused when registering.
    let sc = menus::parse_shortcut("Ctrl+Alt+M").unwrap();
    assert_eq!(sc.logical_key, egui::Key::M);
    assert!(sc.modifiers.command && sc.modifiers.alt && !sc.modifiers.shift);
    assert!(menus::parse_shortcut("Ctrl+Nope").is_none());
    assert!(
        lua(
            &mut s,
            r#"MenuItem("Tools/X", function() end, { shortcut = "Hyper+Q" })"#
        )
        .is_err()
    );
    assert!(lua(&mut s, r#"MenuItem("NoSlash", function() end)"#).is_err());
    // The Ctrl+K search lists them too.
    let cmds = crate::search::menu_items::all(&s);
    let hit = cmds
        .iter()
        .find(|c| c.path == "Tools/Needs Selection")
        .expect("in the search palette");
    assert_eq!(hit.title, "Needs Selection");
    assert_eq!(hit.shortcut, "Ctrl+Alt+M");
    // An item under the old "GameObject" menu name is listed under Object, and still runs its
    // own path.
    let old = cmds
        .iter()
        .find(|c| c.path == "Object/Script Item")
        .expect("listed under Object");
    assert!(matches!(
        &old.run,
        crate::search::menu_items::MenuRun::Action(Action::EditorMenu(p)) if p == "GameObject/Script Item"
    ));
    cleanup(s);
}

#[test]
fn menu_bar_shows_script_menus() {
    let s = project("menubar", &[("Assets/Editor/Tools.lua", TOOLS)]);
    let mut h = Harness::builder()
        .with_size([900.0, 600.0])
        .build_ui_state(|ui, s: &mut EditorState| crate::menu_bar::show(ui, s), s);
    h.run();
    h.get_by_label("Tools").click();
    h.run();
    h.get_by_label("Nested ⏵").hover();
    h.run();
    assert!(h.query_by_label("Deep Item").is_some());
    h.get_by_label("Make Marker").click();
    h.run();
    let actions = std::mem::take(&mut h.state_mut().actions);
    assert!(
        matches!(actions.as_slice(), [Action::EditorMenu(p)] if p == "Tools/Make Marker"),
        "{actions:?}"
    );
    // Items under a built-in menu go at its end.
    // A script item under the old "GameObject" menu name goes to the Object menu.
    h.get_by_label("Object").click();
    h.run();
    assert!(h.query_by_label("Script Item").is_some());
    let s = h.into_state();
    cleanup(s);
}

#[test]
fn errors_name_the_file_and_line_and_never_stop_the_editor() {
    let mut s = project(
        "errors",
        &[
            (
                "Assets/Editor/A_Load.lua",
                "local x = 1\n\nerror('broken on purpose')\n",
            ),
            ("Assets/Editor/B_Syntax.lua", "local = 3\n"),
            (
                "Assets/Editor/C_Menu.lua",
                "MenuItem('Tools/Fails', function()\n    local t = nil\n    return t.field\nend)\n",
            ),
            (
                "Assets/Editor/D_Api.lua",
                "MenuItem('Tools/Bad Arg', function()\n    GUI.Label('outside OnGUI')\nend)\n",
            ),
        ],
    );
    let errs = errors(&s);
    assert!(
        errs.iter()
            .any(|e| e.starts_with("Assets/Editor/A_Load.lua:3:")),
        "{errs:?}"
    );
    assert!(
        errs.iter()
            .any(|e| e.starts_with("Assets/Editor/B_Syntax.lua:1:")),
        "{errs:?}"
    );
    // The files after a broken one still load.
    assert_eq!(s.editor_scripts.menus.len(), 2);
    menus::run(&mut s, "Tools/Fails");
    let last = errors(&s).last().cloned().unwrap();
    assert!(last.starts_with("Assets/Editor/C_Menu.lua:3:"), "{last}");
    menus::run(&mut s, "Tools/Bad Arg");
    let last = errors(&s).last().cloned().unwrap();
    assert!(last.starts_with("Assets/Editor/D_Api.lua:2:"), "{last}");
    assert!(last.contains("only work inside OnGUI"), "{last}");
    // A runaway loop is stopped by the time budget.
    std::fs::write(
        s.project.root.join("Assets/Editor/E_Loop.lua"),
        "MenuItem('Tools/Spin', function()\n    while true do end\nend)\n",
    )
    .unwrap();
    reload(&mut s);
    let started = std::time::Instant::now();
    menus::run(&mut s, "Tools/Spin");
    assert!(started.elapsed() < CALL_BUDGET + Duration::from_secs(5));
    let last = errors(&s).last().cloned().unwrap();
    assert!(last.contains("stopped after"), "{last}");
    assert!(last.starts_with("Assets/Editor/E_Loop.lua:2:"), "{last}");
    // The state still works afterwards.
    assert_eq!(eval(&mut s, "1 + 1"), "2");
    cleanup(s);
}

const COUNTER: &str = r#"
local Counter = ToolWindow("Counter")
Counter.count = 0
Counter.name = "box"
Counter.open = true
function Counter:OnGUI()
    counter_self = self
    GUI.Label("Clicked " .. self.count .. " times")
    if GUI.Button("Add one") then
        self.count = self.count + 1
    end
    self.open = GUI.Foldout("Options", self.open)
    if self.open then
        GUI.Horizontal(function()
            self.name = GUI.TextField("Name", self.name)
        end)
        self.flag = GUI.Toggle("Flag", self.flag or false)
        self.amount = GUI.Slider("Amount", self.amount or 0.5, 0, 1)
        self.whole = GUI.IntField("Whole", self.whole or 2)
        self.real = GUI.FloatField("Real", self.real or 1.5)
        self.tint = GUI.ColorField("Tint", self.tint or Color.red)
        self.at = GUI.Vector3Field("At", self.at or Vector3.one)
        GUI.Scroll(function() GUI.Label("Scrolled") end)
    end
    GUI.Separator()
    GUI.HelpBox("Help text", "warning")
end
MenuItem("Tools/Counter", function() Counter:Open() end)
"#;

fn window_harness(s: EditorState, id: u64) -> Harness<'static, EditorState> {
    Harness::builder()
        .with_size([500.0, 500.0])
        .build_ui_state(move |ui, s: &mut EditorState| windows::show(ui, s, id), s)
}

#[test]
fn tool_window_draws_clicks_and_keeps_its_state() {
    let s = project("window", &[("Assets/Editor/Counter.lua", COUNTER)]);
    assert!(errors(&s).is_empty(), "{:?}", errors(&s));
    let id = window_id("Counter");
    assert_eq!(s.editor_scripts.window(id).unwrap().title, "Counter");
    let mut h = window_harness(s, id);
    h.run();
    assert!(h.query_by_label("Clicked 0 times").is_some());
    h.get_by_label("Add one").click();
    h.run();
    h.run();
    assert!(h.query_by_label("Clicked 1 times").is_some());
    assert!(h.query_by_label("Scrolled").is_some());
    assert!(h.query_by_label("Help text").is_some());
    // Folding hides the options.
    h.get_by_label("⏷ Options").click();
    h.run();
    h.run();
    assert!(h.query_by_label("Scrolled").is_none());
    let mut s = h.into_state();
    assert!(errors(&s).is_empty(), "{:?}", errors(&s));
    assert_eq!(eval(&mut s, "counter_self.count"), "1");
    assert_eq!(eval(&mut s, "counter_self.open"), "false");
    // Opening goes through the dock.
    menus::run(&mut s, "Tools/Counter");
    assert!(matches!(
        s.actions.as_slice(),
        [Action::FocusTab(crate::layout::Tab::Tool(i))] if *i == id
    ));
    // The state survives a reload and is written to the Library folder.
    reload(&mut s);
    let mut h = window_harness(s, id);
    h.run();
    assert!(h.query_by_label("Clicked 1 times").is_some());
    let s = h.into_state();
    let saved = std::fs::read_to_string(s.project.root.join("Library/ToolWindows.json")).unwrap();
    assert!(saved.contains("\"count\": 1"), "{saved}");
    let fresh = windows::WindowStore::load(&s.project.root);
    assert_eq!(fresh.states["Counter"]["name"], "box");
    cleanup(s);
}

#[test]
fn a_runaway_on_gui_is_stopped_and_can_be_retried() {
    let s = project(
        "runaway",
        &[(
            "Assets/Editor/Spin.lua",
            "local W = ToolWindow('Spin')\nfunction W:OnGUI()\n    while true do end\nend\n",
        )],
    );
    let id = window_id("Spin");
    let mut h = window_harness(s, id);
    let started = std::time::Instant::now();
    h.run();
    assert!(started.elapsed() < Duration::from_secs(10));
    h.run();
    assert!(h.query_by_label("Try again").is_some());
    let errs = errors(h.state());
    assert_eq!(errs.len(), 1, "logged once: {errs:?}");
    assert!(errs[0].starts_with("Assets/Editor/Spin.lua:3:"), "{errs:?}");
    // More frames do not log again.
    h.run();
    assert_eq!(errors(h.state()).len(), 1);
    let s = h.into_state();
    cleanup(s);
}

const ORBITER: &str = r#"
local Orbiter = { center = Vector3.new(0, 1, 0), radius = 3, speed = 45, secret = 7 }
Orbiter._attributes = {
    radius = { Header("Orbit"), Range(0.5, 20), Tooltip("Metres") },
    secret = { HideInInspector() },
}
function Orbiter:OnDrawGizmos()
    Gizmos.color = Color.new(1, 0, 0)
    Gizmos.Line(self.center, self.transform.position)
    Gizmos.Icon(self.center, "+")
end
function Orbiter:OnDrawGizmosSelected()
    Gizmos.WireSphere(self.center, self.radius)
    Gizmos.WireCube(self.center, Vector3.one)
    Gizmos.Label(self.center, "centre")
end
return Orbiter
"#;

const ORBITER_INSPECTOR: &str = r#"
local I = CustomInspector("Orbiter")
function I:OnInspectorGUI()
    DrawDefaultInspector()
    GUI.Label("Lap: " .. (360 / self.target.speed) .. " s")
    if GUI.Button("Double radius") then
        self.target.radius = self.target.radius * 2
    end
end
function I:OnSceneGUI()
    scene_gui_runs = (scene_gui_runs or 0) + 1
    self.target.center = Handles.Position(self.target.center)
    Handles.Label(self.target.center, "centre")
end
"#;

fn orbiter_project(name: &str) -> (EditorState, Entity) {
    let mut s = project(
        name,
        &[
            ("Assets/Scripts/Orbiter.lua", ORBITER),
            ("Assets/Editor/OrbiterInspector.lua", ORBITER_INSPECTOR),
        ],
    );
    assert!(errors(&s).is_empty(), "{:?}", errors(&s));
    let e = s.scene.spawn("Planet", None);
    attach(&mut s, e, "Assets/Scripts/Orbiter.lua");
    (s, e)
}

fn radius(s: &EditorState, e: Entity) -> Option<ScriptValue> {
    s.scene
        .world
        .get::<&Scripts>(e)
        .ok()
        .and_then(|sc| sc.list[0].fields.get("radius").cloned())
}

#[test]
fn custom_inspector_extends_the_default_one_with_undoable_edits() {
    let (mut s, e) = orbiter_project("inspector");
    assert!(s.editor_scripts.inspectors.contains("Orbiter"));
    s.select(e);
    s.inspector_targets = vec![e];
    let mut h = Harness::builder().with_size([420.0, 700.0]).build_ui_state(
        |ui, s: &mut EditorState| {
            // As the editor frame does, with every frame counted as mid-interaction.
            s.undo.begin_frame(true, &s.scene, &s.selection, || None);
            crate::panels::inspector_show(ui, s)
        },
        s,
    );
    h.run();
    h.run();
    // The default fields (with attributes) and the custom controls.
    assert!(h.query_by_label("Radius").is_some());
    assert!(h.query_by_label("Orbit").is_some(), "Header attribute");
    assert!(h.query_by_label("Secret").is_none(), "HideInInspector");
    assert!(h.query_by_label("Lap: 8.0 s").is_some());
    h.get_by_label("Double radius").click();
    h.run();
    h.run();
    let mut s = h.into_state();
    assert!(errors(&s).is_empty(), "{:?}", errors(&s));
    assert_eq!(radius(&s, e), Some(ScriptValue::Number(6.0)));
    // The edit is one undo step named after the field.
    s.undo.end_frame(false);
    assert_eq!(s.undo.undo_label(), Some("Change Radius"));
    s.undo_step();
    assert_eq!(radius(&s, e), None);
    // Wrong types are refused with a clear message.
    let err = call(&mut s, CALL_BUDGET, |lua| {
        let inst = inspector::instance(lua, e, 0, "Orbiter", "Assets/Scripts/Orbiter.lua")?;
        lua.load("local i = ...; i.target.radius = 'far'")
            .call::<()>(inst)
    })
    .unwrap()
    .unwrap_err();
    assert!(
        describe(&err).contains("'radius' takes a number"),
        "{}",
        describe(&err)
    );
    cleanup(s);
}

#[test]
fn gizmos_draw_for_every_object_and_more_when_selected() {
    let (mut s, e) = orbiter_project("gizmos");
    s.scene
        .set_world_position(e, glam::Vec3::new(3.0, 1.0, 0.0));
    s.selection.clear();
    let mut lines = crate::gizmos::Lines::default();
    gizmos::lines(&mut s, &mut lines);
    assert_eq!(lines.0.len(), 2, "one line (two vertices)");
    let f = &s.editor_scripts.gizmo_frame;
    assert_eq!(f.icons.len(), 1);
    assert_eq!(f.icons[0].2, [1.0, 0.0, 0.0, 1.0]);
    assert!(f.labels.is_empty());
    s.selection.set(e);
    let mut lines = crate::gizmos::Lines::default();
    gizmos::lines(&mut s, &mut lines);
    // + three 32-segment circles and a 12-edge cube.
    assert_eq!(lines.0.len(), 2 * (1 + 96 + 12));
    assert_eq!(s.editor_scripts.gizmo_frame.labels.len(), 1);
    // A gizmo error is logged once and turns that script's gizmos off.
    std::fs::write(
        s.project.root.join("Assets/Scripts/Orbiter.lua"),
        "local O = {}\nfunction O:OnDrawGizmos()\n    Gizmos.Line(nil, nil)\nend\nreturn O\n",
    )
    .unwrap();
    api::forget_game_class(
        s.editor_scripts.lua.as_ref().unwrap(),
        "Assets/Scripts/Orbiter.lua",
    );
    for _ in 0..3 {
        gizmos::lines(&mut s, &mut crate::gizmos::Lines::default());
    }
    let errs = errors(&s);
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(
        errs[0].starts_with("Assets/Scripts/Orbiter.lua:3:"),
        "{errs:?}"
    );
    // Gizmos outside OnDrawGizmos are refused.
    assert!(lua(&mut s, "Gizmos.Line(Vector3.zero, Vector3.one)").is_err());
    cleanup(s);
}

#[test]
fn position_handle_drags_along_an_axis() {
    let (mut s, e) = orbiter_project("handles");
    s.select(e);
    let camera = s.scene_view.camera.camera();
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let center = glam::Vec3::new(0.0, 1.0, 0.0);
    let origin = crate::picking::to_screen(&camera, rect, center).unwrap();
    let len = camera.world_per_pixel(center, rect.height()) * 80.0;
    let tip = crate::picking::to_screen(&camera, rect, center + glam::Vec3::X * len).unwrap();
    let grab = origin + (tip - origin) * 0.6;
    let mut h = Harness::builder().with_size([800.0, 600.0]).build_ui_state(
        move |ui, s: &mut EditorState| {
            let r = ui.allocate_rect(rect, egui::Sense::click_and_drag());
            handles::scene_gui(ui, s, &camera, rect, r.hovered());
        },
        s,
    );
    let button = |pos: egui::Pos2, pressed: bool| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    h.event(egui::Event::PointerMoved(grab));
    h.run();
    h.event(button(grab, true));
    h.run();
    let to = grab + (tip - origin) * 0.5;
    h.event(egui::Event::PointerMoved(to));
    h.run();
    h.event(button(to, false));
    h.run();
    let mut s = h.into_state();
    assert!(errors(&s).is_empty(), "{:?}", errors(&s));
    let moved = s.scene.world.get::<&Scripts>(e).unwrap().list[0]
        .fields
        .get("center")
        .cloned();
    let Some(ScriptValue::Vector3([x, y, z])) = moved else {
        panic!("centre not moved: {moved:?}");
    };
    // Half an axis length along X, nothing on the others.
    assert!((x - len * 0.5).abs() < len * 0.05, "x = {x}, axis = {len}");
    assert!((y - 1.0).abs() < 1e-3 && z.abs() < 1e-3);
    assert!(eval(&mut s, "scene_gui_runs").parse::<u32>().unwrap() >= 4);
    cleanup(s);
}

#[test]
fn undo_records_one_step_per_call_and_groups_span_calls() {
    let mut s = project(
        "undo",
        &[(
            "Assets/Editor/Undo.lua",
            r#"
MenuItem("Tools/Three Objects", function()
    for i = 1, 3 do
        Undo.Record(nil, "Make Three")
        EditorScene.Create("Thing " .. i)
    end
end)
"#,
        )],
    );
    let before = s.undo.steps().len();
    menus::run(&mut s, "Tools/Three Objects");
    assert_eq!(s.undo.steps().len(), before + 1);
    assert_eq!(s.undo.undo_label(), Some("Make Three"));
    assert!(s.scene_dirty);
    assert_eq!(
        names(&s).iter().filter(|n| n.starts_with("Thing")).count(),
        3
    );
    s.undo_step();
    assert_eq!(
        names(&s).iter().filter(|n| n.starts_with("Thing")).count(),
        0
    );
    // A group spans several calls (an update callback working over frames).
    lua(&mut s, "Undo.BeginGroup('Build Row')").unwrap();
    for i in 0..3 {
        lua(
            &mut s,
            &format!("Undo.Record(nil, 'ignored'); EditorScene.Create('Row {i}')"),
        )
        .unwrap();
    }
    lua(&mut s, "Undo.EndGroup()").unwrap();
    assert_eq!(s.undo.undo_label(), Some("Build Row"));
    s.undo_step();
    assert_eq!(names(&s).iter().filter(|n| n.starts_with("Row")).count(), 0);
    // Undo.Undo / Redo queue the editor's own commands.
    lua(&mut s, "Undo.Undo(); Undo.Redo()").unwrap();
    assert!(matches!(s.actions.as_slice(), [Action::Undo, Action::Redo]));
    cleanup(s);
}

#[test]
fn scripts_reload_when_they_change_and_new_ones_appear() {
    let mut s = project(
        "reload",
        &[(
            "Assets/Editor/Live.lua",
            "MenuItem('Tools/Old', function() end)\n",
        )],
    );
    frame(&mut s);
    assert_eq!(s.editor_scripts.loads, 1);
    let rel = "Assets/Editor/Live.lua";
    let abs = s.project.root.join(rel);
    std::fs::write(&abs, "MenuItem('Tools/New', function() end)\n").unwrap();
    // A later file time, whatever the clock resolution.
    let later = std::time::SystemTime::now() + Duration::from_secs(5);
    std::fs::File::options()
        .write(true)
        .open(&abs)
        .unwrap()
        .set_modified(later)
        .unwrap();
    s.editor_scripts.last_poll = None;
    frame(&mut s);
    assert_eq!(s.editor_scripts.loads, 2);
    let paths: Vec<&str> = s
        .editor_scripts
        .menus
        .iter()
        .map(|m| m.path.as_str())
        .collect();
    assert_eq!(paths, ["Tools/New"]);
    assert!(
        s.console
            .entries
            .iter()
            .any(|e| e.message == format!("Editor scripts reloaded ({rel} changed)."))
    );
    // A new file in another Editor folder is found by the rescan.
    write(
        &s.project.root,
        "Assets/Tools/Editor/More.lua",
        "MenuItem('Tools/More', function() end)\n",
    );
    s.editor_scripts.last_poll = None;
    s.editor_scripts.last_scan = None;
    frame(&mut s);
    assert_eq!(s.editor_scripts.loads, 3);
    assert_eq!(s.editor_scripts.menus.len(), 2);
    // Editor scripts are not components, and never ship.
    assert!(!s.assets.scripts.contains_key(rel));
    assert!(is_editor_script("Assets/Tools/Editor/More.lua"));
    assert!(!is_editor_script("Assets/Scripts/Editorial.lua"));
    cleanup(s);
}

#[test]
fn selection_prefs_assets_scene_dialogs_progress_and_updates() {
    let mut s = project(
        "api",
        &[(
            "Assets/Editor/Hooks.lua",
            r#"
local P = AssetProcessor()
imported = {}
function P:OnPostprocessAsset(path) table.insert(imported, path) end
"#,
        )],
    );
    // Selection.
    lua(&mut s, "Selection.active = EditorScene.Find('Main Camera')").unwrap();
    let cam = s.scene.find_by_name("Main Camera").unwrap();
    assert_eq!(s.selection.active(), Some(cam));
    assert_eq!(eval(&mut s, "Selection.active.name"), "Main Camera");
    assert_eq!(eval(&mut s, "#Selection.objects"), "1");
    lua(&mut s, "Selection.objects = EditorScene.Roots()").unwrap();
    assert_eq!(s.selection.list().len(), s.scene.roots().len());
    lua(&mut s, "Selection.Clear()").unwrap();
    assert!(s.selection.is_empty());
    // EditorPrefs persist in the prefs file.
    lua(
        &mut s,
        "EditorPrefs.SetInt('n', 4); EditorPrefs.SetString('s', 'hi'); EditorPrefs.SetBool('b', true); EditorPrefs.SetFloat('f', 0.5)",
    )
    .unwrap();
    assert_eq!(
        eval(
            &mut s,
            "EditorPrefs.GetInt('n') + EditorPrefs.GetFloat('f')"
        ),
        "4.5"
    );
    assert_eq!(
        eval(&mut s, "EditorPrefs.GetString('missing', 'dflt')"),
        "dflt"
    );
    let saved = std::fs::read_to_string(s.project.root.join("prefs.json")).unwrap();
    assert!(saved.contains("\"s\": \"hi\""), "{saved}");
    lua(&mut s, "EditorPrefs.DeleteKey('n')").unwrap();
    assert_eq!(eval(&mut s, "EditorPrefs.HasKey('n')"), "false");
    // Assets: create, find, read; writes outside Assets are refused; the processor runs.
    lua(
        &mut s,
        "AssetDB.CreateAsset('Assets/Data/notes.txt', 'hello')",
    )
    .unwrap();
    lua(
        &mut s,
        "AssetDB.CreateMaterial('Assets/Data/Red', Color.red)",
    )
    .unwrap();
    assert_eq!(
        eval(&mut s, "AssetDB.ReadText('Assets/Data/notes.txt')"),
        "hello"
    );
    assert_eq!(
        eval(&mut s, "AssetDB.Find('t:txt')[1]"),
        "Assets/Data/notes.txt"
    );
    assert_eq!(
        eval(&mut s, "AssetDB.Find('t:material', 'Assets/Data')[1]"),
        "Assets/Data/Red.mat"
    );
    assert_eq!(eval(&mut s, "#AssetDB.Find('red')"), "1");
    assert!(lua(&mut s, "AssetDB.CreateAsset('../evil.txt', 'x')").is_err());
    assert!(lua(&mut s, "AssetDB.CreateAsset('Assets/../evil.txt', 'x')").is_err());
    assert!(s.assets.materials.contains_key("Assets/Data/Red.mat"));
    frame(&mut s);
    assert_eq!(eval(&mut s, "#imported"), "2");
    super::api::postprocess(&mut s, &["Assets/Textures/new.png".into()]);
    assert_eq!(eval(&mut s, "imported[3]"), "Assets/Textures/new.png");
    // Scene queries and components.
    lua(
        &mut s,
        "local g = EditorScene.Create('Crate'); EditorScene.AddComponent(g, 'Rigidbody'); \
         EditorScene.AddComponent(g, 'Assets/Scripts/Spin.lua'); \
         EditorScene.SetParent(g, EditorScene.Find('Main Camera'))",
    )
    .unwrap();
    assert_eq!(eval(&mut s, "#EditorScene.FindAll('t:Rigidbody')"), "1");
    assert_eq!(eval(&mut s, "#EditorScene.FindAll('t:Spin')"), "1");
    assert_eq!(eval(&mut s, "#EditorScene.FindAll('cra')"), "1");
    assert_eq!(
        eval(
            &mut s,
            "EditorScene.HasComponent(EditorScene.Find('Crate'), 'Light')"
        ),
        "false"
    );
    let crate_e = s.scene.find_by_name("Crate").unwrap();
    assert_eq!(s.scene.parent(crate_e), Some(cam));
    assert!(
        lua(
            &mut s,
            "EditorScene.AddComponent(EditorScene.Find('Crate'), 'Warp Drive')"
        )
        .is_err()
    );
    lua(&mut s, "EditorScene.Destroy(EditorScene.Find('Crate'))").unwrap();
    assert!(s.scene.find_by_name("Crate").is_none());
    // The game API works on the edited scene too.
    lua(
        &mut s,
        "GameObject.Find('Main Camera').transform.position = Vector3.new(1, 2, 3)",
    )
    .unwrap();
    assert_eq!(s.scene.world_position(cam), glam::Vec3::new(1.0, 2.0, 3.0));
    lua(&mut s, "Debug.Log('from an editor script')").unwrap();
    assert_eq!(s.console.last().unwrap().message, "from an editor script");
    // Dialogs (answers queued by the test).
    if let DialogMode::Scripted { answers, .. } = &mut s.editor_scripts.dialogs {
        answers.push_back(false);
    }
    assert_eq!(
        eval(
            &mut s,
            "Editor.DisplayDialog('Delete?', 'Sure?', 'Yes', 'No')"
        ),
        "false"
    );
    assert_eq!(
        eval(&mut s, "Editor.DisplayDialog('Done', 'All good')"),
        "true"
    );
    let DialogMode::Scripted { shown, .. } = &s.editor_scripts.dialogs else {
        panic!("tests use scripted dialogs");
    };
    assert_eq!(shown[0], ("Delete?".to_owned(), "Sure?".to_owned()));
    // Progress and update callbacks.
    lua(&mut s, "Editor.ShowProgress('Baking', 'step 1', 0.25)").unwrap();
    assert_eq!(s.editor_scripts.progress.as_ref().unwrap().fraction, 0.25);
    lua(&mut s, "Editor.ClearProgress()").unwrap();
    assert!(s.editor_scripts.progress.is_none());
    lua(
        &mut s,
        "ticks = 0; Editor.AddUpdate(function() ticks = ticks + 1; if ticks == 3 then return false end end)",
    )
    .unwrap();
    for _ in 0..5 {
        frame(&mut s);
    }
    assert_eq!(eval(&mut s, "ticks"), "3");
    assert_eq!(eval(&mut s, "Editor.IsPlaying()"), "false");
    assert!(errors(&s).is_empty(), "{:?}", errors(&s));
    cleanup(s);
}

/// The SampleProject's editor scripts load cleanly, and the Scatter Tool scatters a prefab.
#[test]
fn sample_project_editor_scripts_work() {
    let sample = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject/Assets");
    let read = |rel: &str| std::fs::read_to_string(sample.join(rel)).unwrap();
    let mut s = project(
        "sample",
        &[
            (
                "Assets/Editor/ScatterTool.lua",
                &read("Editor/ScatterTool.lua"),
            ),
            (
                "Assets/Editor/SatelliteInspector.lua",
                &read("Editor/SatelliteInspector.lua"),
            ),
            (
                "Assets/Scripts/Satellite.lua",
                &read("Scripts/Satellite.lua"),
            ),
        ],
    );
    assert!(errors(&s).is_empty(), "{:?}", errors(&s));
    assert_eq!(s.editor_scripts.files().count(), 2);
    let paths: Vec<&str> = s
        .editor_scripts
        .menus
        .iter()
        .map(|m| m.path.as_str())
        .collect();
    assert_eq!(paths, ["Tools/Scatter Tool", "Tools/Align to Floor"]);
    assert!(s.editor_scripts.inspectors.contains("Satellite"));
    // A prefab to scatter.
    let mut scene = misty_engine::scene::Scene::default_3d("P");
    let ball = scene.spawn("Ball", None);
    misty_engine::scene::prefab::Prefab::from_scene(&scene, ball)
        .save(&s.project.root.join("Assets/Ball.prefab"))
        .unwrap();
    let id = window_id("Scatter Tool");
    let scattered = call(&mut s, CALL_BUDGET, |lua| {
        let w = windows::instance(lua, id).expect("window instance");
        w.set("prefab", "Assets/Ball.prefab")?;
        w.set("count", 7)?;
        let f: mlua::Function = w.get("Scatter")?;
        f.call::<()>((w, mlua::Value::Nil))
    })
    .unwrap();
    assert!(
        scattered.is_ok(),
        "{:?}",
        scattered.map_err(|e| describe(&e))
    );
    let group = s.scene.find_by_name("Scattered").expect("group made");
    assert_eq!(s.scene.children(group).len(), 7);
    assert_eq!(s.selection.active(), Some(group));
    assert_eq!(s.undo.undo_label(), Some("Scatter Prefabs"));
    for c in s.scene.children(group) {
        let p = s.scene.world_position(c);
        assert!(p.y.abs() < 1e-4 && p.length() <= 5.0 + 1e-3, "{p}");
        assert_eq!(
            s.scene.prefab_link(c).as_deref(),
            Some("Assets/Ball.prefab")
        );
    }
    // Align to Floor works on the selection.
    s.selection.set_many(s.scene.children(group).to_vec());
    lua(
        &mut s,
        "for _, g in ipairs(Selection.objects) do g.transform.position = Vector3.new(0, 4, 0) end",
    )
    .unwrap();
    assert!(menus::run(&mut s, "Tools/Align to Floor"));
    assert!(
        s.scene
            .children(group)
            .iter()
            .all(|c| s.scene.world_position(*c).y.abs() < 1e-4)
    );
    // Satellite gizmos and its Inspector draw without errors.
    let e = s.scene.spawn("Moon", None);
    attach(&mut s, e, "Assets/Scripts/Satellite.lua");
    s.select(e);
    s.inspector_targets = vec![e];
    gizmos::lines(&mut s, &mut crate::gizmos::Lines::default());
    assert!(s.editor_scripts.gizmo_frame.lines.len() > 48);
    let mut h = Harness::builder().with_size([420.0, 700.0]).build_ui_state(
        |ui, s: &mut EditorState| crate::panels::inspector_show(ui, s),
        s,
    );
    h.run();
    h.run();
    assert!(h.query_by_label("Centre on origin").is_some());
    assert!(h.query_by_label("Radius").is_some());
    let s = h.into_state();
    assert!(errors(&s).is_empty(), "{:?}", errors(&s));
    cleanup(s);
}

#[test]
fn game_script_gizmos_draw_without_any_editor_script() {
    let mut s = project("gizmos_only", &[("Assets/Scripts/Orbiter.lua", ORBITER)]);
    assert!(s.editor_scripts.menus.is_empty());
    let e = s.scene.spawn("Planet", None);
    attach(&mut s, e, "Assets/Scripts/Orbiter.lua");
    let mut lines = crate::gizmos::Lines::default();
    gizmos::lines(&mut s, &mut lines);
    assert_eq!(lines.0.len(), 2);
    // A game script that fails to load in the editor stays quiet (the game reports it).
    let other = s.scene.spawn("Broken", None);
    write(
        &s.project.root,
        "Assets/Scripts/Broken.lua",
        "error('only at run time')\n",
    );
    attach(&mut s, other, "Assets/Scripts/Broken.lua");
    gizmos::lines(&mut s, &mut crate::gizmos::Lines::default());
    assert!(errors(&s).is_empty(), "{:?}", errors(&s));
    cleanup(s);
}
