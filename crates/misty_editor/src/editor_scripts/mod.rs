//! Editor scripting in Lua: scripts in any `Editor` folder under `Assets` extend the editor with
//! menu items, tool windows, custom Inspectors, scene gizmos and handles, and asset hooks.
//!
//! Editor scripts run in their own Lua state, never in builds (`asset_deps` leaves `Editor`
//! folders out), and reload when any of them changes on disk. The state has the whole game API
//! too, so `GameObject`, `transform`, `Vector3` and `Debug.Log` work as in game scripts, on the
//! scene being edited.
//!
//! Every call into Lua goes through [`call`]: the editor's scene moves into the Lua state's
//! [`World`] for the call, the [`EditorState`] is reachable from API functions through a pointer
//! that only lives for the call, and a time budget stops runaway loops. Errors go to the Console
//! with the script's file and line, and the callback that failed stays off until its script
//! changes (or the user presses Try again), so one bad `OnGUI` never floods the Console.

pub mod api;
pub mod gizmos;
mod gui;
pub mod handles;
pub mod inspector;
pub mod menus;
mod prefs;
mod scene_api;
pub mod windows;

#[cfg(test)]
mod tests;

use std::cell::Cell;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use misty_engine::scene::Scene;
use misty_engine::scripting::{LogKind, World};
use mlua::{HookTriggers, Lua, VmState};

use crate::console::LogLevel;
use crate::state::EditorState;

/// Time a per-frame callback (`OnGUI`, `OnInspectorGUI`, gizmos, handles) may take.
pub const FRAME_BUDGET: Duration = Duration::from_millis(500);
/// Time a one-off callback (a menu item, a script file loading, an asset hook) may take.
pub const CALL_BUDGET: Duration = Duration::from_secs(5);
/// Memory the editor Lua state may use.
const MEMORY_LIMIT: usize = 512 * 1024 * 1024;
/// How often script files are checked for changes, and the folders rescanned for new ones.
const POLL: Duration = Duration::from_millis(500);
const SCAN: Duration = Duration::from_secs(2);

/// A menu item as the menu bar and the search palette see it.
#[derive(Clone, Debug, PartialEq)]
pub struct MenuInfo {
    /// "Tools/Scatter Tool".
    pub path: String,
    /// As written ("Ctrl+Shift+T"), empty when none.
    pub shortcut: String,
    pub has_validate: bool,
    /// Sort order within its menu (lower first), then registration order.
    pub priority: i32,
    pub file: String,
}

/// A tool window registered by a script.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowInfo {
    /// Stable id from the title (the dock layout stores it).
    pub id: u64,
    pub title: String,
    pub file: String,
}

/// `Editor.ShowProgress` while it is up.
#[derive(Clone, Debug, PartialEq)]
pub struct Progress {
    pub title: String,
    pub info: String,
    pub fraction: f32,
}

/// How `Editor.DisplayDialog` answers: a native message box, or answers queued by tests.
#[derive(Debug, Default)]
pub enum DialogMode {
    #[default]
    Native,
    /// Tests: each dialog takes the next answer (OK when the queue is empty) and is recorded.
    Scripted {
        answers: std::collections::VecDeque<bool>,
        shown: Vec<(String, String)>,
    },
}

/// Editor scripts: the Lua state and what the scripts registered, in plain form.
#[derive(Default)]
pub struct EditorScripts {
    lua: Option<Lua>,
    root: PathBuf,
    /// Loaded editor scripts and the file times they were loaded at.
    files: BTreeMap<String, Option<SystemTime>>,
    /// Game scripts read for gizmos and Inspectors, with their file times.
    game_files: BTreeMap<String, Option<SystemTime>>,
    started: bool,
    last_poll: Option<Instant>,
    last_scan: Option<Instant>,
    pub menus: Vec<MenuInfo>,
    pub windows: Vec<WindowInfo>,
    /// Script names (file stems) that have a custom Inspector.
    pub inspectors: HashSet<String>,
    /// Callbacks turned off after an error, until their script changes ("window:Scatter Tool").
    broken: HashSet<String>,
    /// What the gizmo pass drew this frame (lines, icons and labels).
    pub gizmo_frame: gizmos::GizmoFrame,
    pub progress: Option<Progress>,
    pub dialogs: DialogMode,
    prefs: prefs::EditorPrefs,
    pub(crate) window_store: windows::WindowStore,
    /// Undo grouping: whether this call (or the open group) has recorded already.
    undo: UndoGroup,
    /// A script asked for a repaint (`Repaint()`, an update callback running).
    repaint: bool,
    /// Times the scripts were (re)loaded (tests).
    pub loads: usize,
}

#[derive(Default)]
struct UndoGroup {
    /// `Undo.BeginGroup` name while a group is open.
    open: Option<String>,
    recorded: bool,
}

/// The editor state while a Lua call runs (see [`with_state`]), and whether it is lent out.
struct StatePtr {
    state: *mut EditorState,
    busy: Cell<bool>,
}

/// Wall-clock limit for the Lua code running now (checked by an instruction-count hook).
struct Deadline {
    at: Cell<Option<Instant>>,
    budget: Cell<Duration>,
}

impl EditorScripts {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            prefs: prefs::EditorPrefs::load(prefs::default_path()),
            window_store: windows::WindowStore::load(root),
            // Tests never open native dialogs.
            dialogs: if cfg!(test) {
                DialogMode::Scripted {
                    answers: Default::default(),
                    shown: Vec::new(),
                }
            } else {
                DialogMode::Native
            },
            ..Default::default()
        }
    }

    /// Whether any editor script is loaded.
    pub fn active(&self) -> bool {
        self.lua.is_some()
    }

    /// The loaded editor script files (project-relative).
    #[cfg(test)]
    pub fn files(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(String::as_str)
    }

    pub fn window(&self, id: u64) -> Option<&WindowInfo> {
        self.windows.iter().find(|w| w.id == id)
    }

    fn is_broken(&self, site: &str) -> bool {
        self.broken.contains(site)
    }

    /// Turn a failed callback back on (the Try again buttons).
    pub fn retry(&mut self, site: &str) {
        self.broken.remove(site);
    }

    /// Keep EditorPrefs in a test file instead of the user's.
    #[cfg(test)]
    pub fn use_prefs_file(&mut self, path: PathBuf) {
        self.prefs = prefs::EditorPrefs::load(Some(path));
    }
}

/// Stable id of a tool window title (FNV-1a), stored in saved layouts.
pub fn window_id(title: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in title.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// Run `f` with the editor state of the running call.
///
/// SAFETY: the pointer is set by [`call`] from its `&mut EditorState` and removed before
/// `call` returns; `call` does not touch the state while Lua runs. The `busy` flag makes this
/// the only live reference: a nested `with_state` (from Lua called inside `f`) is an error
/// instead of a second `&mut`.
pub(crate) fn with_state<R>(lua: &Lua, f: impl FnOnce(&mut EditorState) -> R) -> mlua::Result<R> {
    let p = {
        let cell = lua
            .app_data_ref::<StatePtr>()
            .ok_or_else(|| mlua::Error::runtime("the editor is not available here"))?;
        if cell.busy.replace(true) {
            return Err(mlua::Error::runtime(
                "editor API called from inside another editor API call",
            ));
        }
        cell.state
    };
    let r = f(unsafe { &mut *p });
    if let Some(cell) = lua.app_data_ref::<StatePtr>() {
        cell.busy.set(false);
    }
    Ok(r)
}

/// Run `f` with the editor scene in the Lua state's world and the editor state reachable
/// from API functions. `None` when no editor scripts are loaded (or during another call).
pub(crate) fn call<R>(
    state: &mut EditorState,
    budget: Duration,
    f: impl FnOnce(&Lua) -> R,
) -> Option<R> {
    let lua = state.editor_scripts.lua.take()?;
    api::begin_call(&mut state.editor_scripts);
    let selection_before = state.selection.list().to_vec();
    let scene = std::mem::take(&mut state.scene);
    if let Some(mut w) = lua.app_data_mut::<World>() {
        w.scene = scene;
    }
    lua.set_app_data(StatePtr {
        state: state as *mut EditorState,
        busy: Cell::new(false),
    });
    arm(&lua, Some(budget));
    let result = f(&lua);
    arm(&lua, None);
    lua.remove_app_data::<StatePtr>();
    let (scene, logs) = match lua.app_data_mut::<World>() {
        Some(mut w) => {
            // `Destroy` and `Instantiate` from the game API: objects go now, copies need no
            // script instances in the editor.
            let doomed = std::mem::take(&mut w.destroy);
            for e in doomed {
                if w.scene.contains(e) {
                    w.scene.despawn(e);
                }
            }
            w.spawned.clear();
            w.requests.clear();
            (std::mem::take(&mut w.scene), std::mem::take(&mut w.logs))
        }
        None => (Scene::default(), Vec::new()),
    };
    state.scene = scene;
    for (kind, text) in logs {
        let level = match kind {
            LogKind::Info => LogLevel::Info,
            LogKind::Warning => LogLevel::Warning,
            LogKind::Error => LogLevel::Error,
        };
        state.console.log(level, text);
    }
    state.selection.retain_existing(&state.scene);
    if state.selection.list() != selection_before.as_slice()
        && let Some(e) = state.selection.active()
    {
        state.reveal(e);
    }
    api::sync_registry(&lua, &mut state.editor_scripts);
    state.editor_scripts.lua = Some(lua);
    Some(result)
}

fn arm(lua: &Lua, budget: Option<Duration>) {
    if let Some(d) = lua.app_data_ref::<Deadline>() {
        d.at.set(budget.map(|b| Instant::now() + b));
        if let Some(b) = budget {
            d.budget.set(b);
        }
    }
}

/// Pause the time budget around something that waits for the user (a dialog).
pub(crate) fn paused<R>(lua: &Lua, f: impl FnOnce() -> R) -> R {
    let left = lua
        .app_data_ref::<Deadline>()
        .and_then(|d| d.at.get())
        .map(|t| t.saturating_duration_since(Instant::now()));
    arm(lua, None);
    let r = f();
    if let Some(left) = left
        && let Some(d) = lua.app_data_ref::<Deadline>()
    {
        d.at.set(Some(Instant::now() + left));
    }
    r
}

/// Stop runaway Lua (`while true do end`) instead of freezing the editor.
fn install_deadline(lua: &Lua) {
    lua.set_app_data(Deadline {
        at: Cell::new(None),
        budget: Cell::new(FRAME_BUDGET),
    });
    let _ = lua.set_global_hook(
        HookTriggers::new().every_nth_instruction(10_000),
        |lua, debug| {
            if let Some(d) = lua.app_data_ref::<Deadline>()
                && let Some(t) = d.at.get()
                && Instant::now() > t
            {
                // Some slack so the error can unwind through pcalls.
                d.at.set(Some(Instant::now() + Duration::from_millis(200)));
                let ms = d.budget.get().as_millis();
                // Where it was looping, like any other Lua error.
                let at = debug
                    .source()
                    .short_src
                    .map(|s| format!("{s}:{}: ", debug.current_line().unwrap_or(0)))
                    .unwrap_or_default();
                return Err(mlua::Error::runtime(format!(
                    "{at}stopped after {ms} ms (an endless loop?)"
                )));
            }
            Ok(VmState::Continue)
        },
    );
}

/// A Lua error as one Console line that starts with the script's `file:line:` when it is
/// known (the Console links it to the Code Editor).
pub fn describe(e: &mlua::Error) -> String {
    match e {
        mlua::Error::CallbackError { traceback, cause } => {
            let cause = describe(cause);
            if has_location(&cause) {
                return cause;
            }
            match first_location(traceback) {
                Some(loc) => format!("{loc}: {cause}"),
                None => cause,
            }
        }
        mlua::Error::WithContext { context, cause } => format!("{context}: {}", describe(cause)),
        mlua::Error::SyntaxError { message, .. } => clean(message),
        mlua::Error::RuntimeError(m) => clean(m),
        other => clean(&other.to_string()),
    }
}

/// `[string "Assets/Editor/A.lua"]:3:` → `Assets/Editor/A.lua:3:`; the traceback is dropped.
fn clean(message: &str) -> String {
    let first = message
        .split("\nstack traceback:")
        .next()
        .unwrap_or(message);
    let mut out = first.trim().to_owned();
    while let Some(start) = out.find("[string \"") {
        let rest = &out[start + 9..];
        let Some(end) = rest.find("\"]") else { break };
        let name = rest[..end].to_owned();
        out.replace_range(start..start + 9 + end + 2, &name);
    }
    out
}

fn has_location(message: &str) -> bool {
    first_location(message).is_some_and(|l| message.starts_with(&l))
}

/// The first `Assets/….lua:N` in `text`.
fn first_location(text: &str) -> Option<String> {
    let text = clean_names(text);
    let start = text.find("Assets/")?;
    let rest = &text[start..];
    let lua = rest.find(".lua:")?;
    let digits: String = rest[lua + 5..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    (!digits.is_empty() && !rest[..lua].contains('\n'))
        .then(|| format!("{}.lua:{digits}", &rest[..lua]))
}

fn clean_names(text: &str) -> String {
    text.replace("[string \"", "").replace("\"]", "")
}

/// Log a script error once and turn its callback off until the script changes.
pub(crate) fn fail(state: &mut EditorState, site: &str, err: &mlua::Error) {
    let message = describe(err);
    state.console.log(LogLevel::Error, message);
    state.editor_scripts.broken.insert(site.to_owned());
}

/// Is `rel` an editor script?
pub fn is_editor_script(rel: &str) -> bool {
    rel.ends_with(".lua") && misty_engine::asset_deps::is_editor_only(rel)
}

/// Every editor script under `Assets`, sorted (the load order).
fn scan(root: &Path) -> Vec<String> {
    // The project's GUID database lists every asset (the asset watcher keeps it current from
    // its own thread): walking `Assets/` here took tens of milliseconds every two seconds.
    let listed = misty_engine::asset_db::with(|db| {
        (db.root() == root).then(|| {
            db.paths()
                .filter(|p| is_editor_script(p))
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
    })
    .flatten();
    let mut out: Vec<String> = listed.unwrap_or_else(|| {
        misty_engine::vfs::files_under(&root.join("Assets"))
            .into_iter()
            .filter_map(|p| {
                let rel = p
                    .strip_prefix(root)
                    .ok()?
                    .to_string_lossy()
                    .replace('\\', "/");
                is_editor_script(&rel).then_some(rel)
            })
            .collect()
    });
    out.sort();
    out
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// A fresh Lua state with the game and editor APIs and the editor's world.
fn new_lua(state: &EditorState) -> Result<Lua, String> {
    let lua = Lua::new();
    let _ = lua.set_memory_limit(MEMORY_LIMIT);
    misty_engine::scripting::api::register(&lua).map_err(|e| e.to_string())?;
    api::register(&lua).map_err(|e| e.to_string())?;
    let root = state.project.root.clone();
    let settings = &state.project_settings;
    let empty = Scene::default();
    let physics = misty_engine::physics::Physics::build_with(&empty, &settings.physics);
    let physics2d =
        misty_engine::physics2d::Physics2D::build(&empty, &settings.physics2d, &settings.physics);
    let (audio, _) = misty_engine::audio::Audio::null(root.clone());
    let mut world = World::new(empty, physics, physics2d, root.clone(), audio, settings);
    world.logs.clear();
    lua.set_app_data(world);
    lua.set_app_data(misty_engine::asset_groups::AssetSystem::auto(&root));
    install_deadline(&lua);
    Ok(lua)
}

/// (Re)load every editor script: a fresh Lua state, then each file in path order. Tool
/// windows keep their state; failed callbacks are turned back on.
pub fn reload(state: &mut EditorState) {
    // Window state survives the reload.
    windows::capture_all(state);
    state.editor_scripts.window_store.save(&state.project.root);
    state.editor_scripts.lua = None;
    state.editor_scripts.broken.clear();
    state.editor_scripts.game_files.clear();
    state.editor_scripts.progress = None;
    state.editor_scripts.started = true;
    state.editor_scripts.root = state.project.root.clone();
    let files = scan(&state.project.root);
    state.editor_scripts.files = files
        .iter()
        .map(|rel| (rel.clone(), mtime(&state.project.root.join(rel))))
        .collect();
    // Also without editor scripts: game scripts' gizmos run in this state.
    state.editor_scripts.menus.clear();
    state.editor_scripts.windows.clear();
    state.editor_scripts.inspectors.clear();
    misty_engine::profile_scope!("EditorScripts.Reload");
    let fresh = {
        misty_engine::profile_scope!("EditorScripts.NewLua");
        new_lua(state)
    };
    let lua = match fresh {
        Ok(l) => l,
        Err(e) => {
            state
                .console
                .log(LogLevel::Error, format!("Editor scripts: {e}"));
            return;
        }
    };
    state.editor_scripts.lua = Some(lua);
    state.editor_scripts.loads += 1;
    for rel in files {
        let source = match misty_engine::vfs::read_to_string(state.project.root.join(&rel)) {
            Ok(s) => s,
            Err(e) => {
                state.console.warn(format!("{rel}: {e}"));
                continue;
            }
        };
        let result = call(state, CALL_BUDGET, |lua| {
            api::set_current_file(lua, &rel);
            let r = lua.load(&source).set_name(&rel).exec();
            api::set_current_file(lua, "");
            r
        });
        if let Some(Err(e)) = result {
            state.console.log(LogLevel::Error, describe(&e));
        }
    }
    windows::create_instances(state);
}

/// Save tool window state (the editor is closing).
pub fn on_exit(state: &mut EditorState) {
    windows::capture_all(state);
    state.editor_scripts.window_store.save(&state.project.root);
}

/// Once per editor frame: load on first use, reload when a script changed, run update
/// callbacks and asset hooks. True when the scripts want another frame soon.
pub fn frame(state: &mut EditorState) -> bool {
    if !state.editor_scripts.started {
        reload(state);
    }
    let now = Instant::now();
    let es = &mut state.editor_scripts;
    if es.last_poll.is_none_or(|t| now.duration_since(t) >= POLL) {
        es.last_poll = Some(now);
        let rescan = es.last_scan.is_none_or(|t| now.duration_since(t) >= SCAN);
        if let Some(why) = changed(state, rescan) {
            reload(state);
            state
                .console
                .info(format!("Editor scripts reloaded ({why})."));
        }
    }
    api::run_updates(state);
    let mut imported = std::mem::take(&mut state.assets.imported);
    imported.sort();
    imported.dedup();
    if !imported.is_empty() {
        api::postprocess(state, &imported);
    }
    windows::autosave(state);
    let es = &mut state.editor_scripts;
    std::mem::take(&mut es.repaint) || es.progress.is_some()
}

/// What changed on disk since the scripts loaded, if anything.
fn changed(state: &mut EditorState, rescan: bool) -> Option<String> {
    let root = state.project.root.clone();
    let es = &mut state.editor_scripts;
    for (rel, t) in &es.files {
        let abs = root.join(rel);
        // The background file watch's stamp first; a difference is checked on disk.
        if misty_engine::file_watch::cached_modified(&abs) != *t && mtime(&abs) != *t {
            return Some(format!("{rel} changed"));
        }
    }
    // Game scripts read for gizmos or Inspectors: forget the class, keep the editor scripts.
    let stale: Vec<String> = es
        .game_files
        .iter()
        .filter(|(rel, t)| {
            let abs = root.join(rel);
            misty_engine::file_watch::cached_modified(&abs) != **t && mtime(&abs) != **t
        })
        .map(|(rel, _)| rel.clone())
        .collect();
    for rel in stale {
        es.game_files.remove(&rel);
        es.broken.retain(|site| !site.ends_with(&rel));
        if let Some(lua) = es.lua.as_ref() {
            api::forget_game_class(lua, &rel);
        }
    }
    if rescan {
        es.last_scan = Some(Instant::now());
        let now = scan(&root);
        if now.len() != es.files.len() || now.iter().any(|f| !es.files.contains_key(f)) {
            let added = now.iter().find(|f| !es.files.contains_key(*f));
            return Some(match added {
                Some(f) => format!("{f} added"),
                None => "a script was removed".into(),
            });
        }
    }
    None
}

/// `Editor.ShowProgress`: a small bar at the bottom of the window while it is up.
pub fn progress_ui(ctx: &egui::Context, state: &EditorState) {
    let Some(p) = &state.editor_scripts.progress else {
        return;
    };
    egui::Area::new(egui::Id::new("misty_editor_script_progress"))
        .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -36.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(320.0);
                ui.label(crate::theme::bold(ui.ctx(), p.title.clone()));
                if !p.info.is_empty() {
                    ui.weak(&p.info);
                }
                ui.add(egui::ProgressBar::new(p.fraction).show_percentage());
            });
        });
}
