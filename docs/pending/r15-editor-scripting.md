# Round 15 — Editor scripting in Lua

## Progress

- **Editor scripts** (`misty_editor::editor_scripts`): `.lua` files in any `Editor` folder under
  `Assets` run in the editor's own Lua state (with the whole game API, on the scene being edited),
  load in path order on the first editor frame and reload when one changes or a new one appears
  (checked every 0.5 s / rescanned every 2 s). Builds leave `Editor` folders out
  (`asset_deps::is_editor_only`), and they are not listed as script components.
- **Menu items**: `MenuItem(path, fn [, { validate, shortcut, priority }])`. New top-level menus
  sit between Component and Window, paths under a built-in menu go at its end, deeper paths make
  submenus; `validate` greys the item out; shortcuts ("Ctrl+Alt+G") run after the built-in
  ones; every item is in the Ctrl+K search (`Action::EditorMenu`).
- **Tool windows**: `ToolWindow(title)` classes with `OnGUI` / `OnEnable`, `Open` / `Close` /
  `Repaint`. Each is a dock tab (`Tab::Tool(id)`, the id an FNV hash of the title, so saved
  layouts keep it); plain values on the window are saved to `Library/ToolWindows.json` and come
  back after reloads and restarts.
- **GUI**: `Label`, `Header`, `HelpBox`, `Button`, `Toggle`, `Slider`, `IntField`, `FloatField`,
  `TextField`, `ColorField`, `Vector3Field`, `ObjectField` (scene objects or assets of a kind,
  pick list + drop from the Project window), `Foldout`, `Horizontal`, `Vertical`, `Scroll`,
  `Space`, `Separator`.
- **Custom Inspectors**: `CustomInspector("Script")` with `OnInspectorGUI` (and
  `DrawDefaultInspector()`), `self.target` reading/writing the script's fields as undoable
  Inspector edits (unchanged writes record nothing), `gameObject` / `transform`.
- **Inspector attributes** (`misty_engine::scripting::attributes`): `Header`, `Tooltip`,
  `Space`, `Range` (slider) and `HideInInspector`, listed per field in the class's `_attributes`
  table; read by `inspect_script` into `ScriptMeta::attributes`.
- **Gizmos and handles**: `OnDrawGizmos` / `OnDrawGizmosSelected` on game scripts draw
  `Gizmos.Line / Ray / WireSphere / WireCube / Icon / Label` in `Gizmos.color` in the Scene view
  (edit mode, Gizmos toggle on); `OnSceneGUI` of a custom Inspector runs while the object is the
  active selection, with `Handles.Position(p)` (three-axis drag handle) and `Handles.Label`.
- **Editor API**: `Selection` (`active`, `objects`, `asset`, `count`, `Clear`, `Contains`),
  `Undo` (`Record`, `BeginGroup` / `EndGroup`, `Undo`, `Redo`), `EditorPrefs` (per-user JSON,
  the `PlayerPrefs` calls), `AssetDB` (`Find`, `Exists`, `ReadText`, `CreateAsset`,
  `CreateFolder`, `CreateMaterial`, `Refresh`), `EditorScene` (`Find`, `FindAll`, `Roots`,
  `Create`, `InstantiatePrefab`, `AddComponent`, `HasComponent`, `SetParent`, `Destroy`),
  `Editor` (`DisplayDialog`, `ShowProgress` / `ClearProgress`, `AddUpdate` / `RemoveUpdate`,
  `IsPlaying`, `ProjectPath`, `Repaint`), `AssetProcessor()` with `OnPostprocessAsset(path)`.
- **Safety**: errors go to the Console as `Assets/…lua:line: message` (linked to the Code
  Editor), including errors raised by API functions and stopped loops; a failing callback is
  logged once and stays off until its script changes (windows and Inspectors show Try again); an
  instruction-count hook stops runaway Lua (0.5 s per frame callback, 5 s per one-off call);
  the state has a 512 MB memory limit.
- **Sample**: `SampleProject/Assets/Editor/ScatterTool.lua` (Tools ▸ Scatter Tool window
  scattering a prefab around the selection as one undo step; Tools ▸ Align to Floor with validate
  and shortcut), `Assets/Editor/SatelliteInspector.lua` and `Assets/Scripts/Satellite.lua`
  (attributes, gizmos, custom Inspector, centre handle).
- **Docs**: `docs/scripting.md` ▸ Inspector attributes, Editor scripting.

## Decisions

1. **Names**: `ToolWindow`, `CustomInspector`, `AssetProcessor`, `AssetDB`, `EditorScene`,
   `Editor`, `GUI`, and Misty-shaped gizmo calls (`Gizmos.Line`, `Gizmos.WireSphere`,
   `Handles.Position`). Kept as given by the round's brief: `MenuItem`, `OnGUI`,
   `DrawDefaultInspector`, `OnDrawGizmos(Selected)`, `OnSceneGUI`, `Undo.Record`,
   `DisplayDialog`, `OnPostprocessAsset`; `Selection`, `Undo` and `EditorPrefs` follow the
   existing game API's naming (`PlayerPrefs`). `AssetDB` because the game API already owns
   `Assets` (addressables).
2. **One editor Lua state with the game API, the editor scene lent to it per call.** The scene
   moves into the state's `World` for each call (and back), so `GameObject`, `transform`,
   `GetComponent`, `Instantiate`, `Destroy` and `Debug.Log` work unchanged on the edited scene.
   The world's physics is built from an empty scene: physics queries find nothing in editor
   scripts.
3. **The editor state is reached through a pointer that lives for one call.** `call` takes the
   Lua state out of `EditorScripts` (so a nested call is impossible and nothing aliases it), sets
   a `*mut EditorState` app-data entry, runs, and removes it. API functions use it only through
   `with_state(lua, |st| …)`, which refuses to nest (a busy flag), so there is never a second
   `&mut` and no Lua runs while it is borrowed. The `GUI` functions reach the `Ui`
   the same way (a stack, for layout helpers). Registering scoped functions per frame was the
   alternative; it costs dozens of allocations per window per frame.
4. **Errors switch the callback off until the script changes** (`window:`, `inspector:`,
   `scene:`, `gizmos:`, `validate:` sites), instead of logging every frame. Menu items and
   update callbacks log each time (they are one-offs; an erroring update is removed).
5. **Time budget via an instruction-count hook** (every 10 000 instructions, like the Test
   Runner): 0.5 s for per-frame callbacks, 5 s for one-off calls. The budget pauses while a
   native dialog is open. The error names the file and line that was running.
6. **Undo**: one snapshot per call — the first `Undo.Record` of a call (or of an open group)
   pushes a scene snapshot, later ones in the same call only mark the scene changed.
   `self.target` writes use the Inspector's grouped edits (`mark_edited`), so a handle drag is
   one step.
7. **Field attributes live in a `_attributes` class table** of small tables built by global
   functions. Wrapping the value (`radius = Range(0, 10, 3)`) would change what games see; a
   separate table keeps game code untouched. The functions are in the game API so scripts with
   attributes run in builds.
8. **`DisplayDialog` is a native, blocking message box (rfd)**, like the reference editor's modal
   dialog, so scripts get the answer as a return value. Tests queue answers instead
   (`DialogMode::Scripted`, the default under `cfg(test)`).
9. **Progress bar between frames**: an immediate-mode editor cannot repaint inside a blocking Lua
   loop, so `Editor.ShowProgress` draws a bar at the bottom of the window and long jobs split
   their work with `Editor.AddUpdate` (documented with an example).
10. **`OnPostprocessAsset` runs for new assets**: the GUID watcher's `created` list (files
    copied into `Assets`), assets the editor's refresh gives a new meta, and `AssetDB.Create*`.
    Re-imports of changed files do not call it yet.
11. **The sample game script is `Satellite.lua`**: SampleProject already has an `Orbiter.lua`
    (the audio demo's), which stays as it was. No sample scene was added (scenes are generated
    and checked against their generators); the docs say to add Satellite to any object.
12. **Tool window state in the project's `Library/ToolWindows.json`**, EditorPrefs in the
    per-user folder (`editor_prefs.json` next to `hub.json`).

## Tests

- `editor_scripts::tests`: menu items (registration, order, validate, run, shortcuts, bad
  paths/shortcuts refused, Ctrl+K entries), the menu bar (kittest: Tools menu, submenu, item
  under GameObject, click queues `EditorMenu`), errors with file and line (load, syntax, runtime,
  API misuse, stopped loop) and the state staying usable, a tool window (kittest: label, button,
  foldout, scroll, help box; state kept across reload and saved to `Library`), a runaway `OnGUI`
  (kittest: stopped, logged once, Try again), a custom Inspector (kittest: default fields with
  Header / HideInInspector, custom label and button; the edit is one "Change Radius" undo step;
  wrong types refused), gizmos (counts per call, selected-only shapes, colour, error logged once,
  refused outside the callback), the position handle (kittest drag along X moves the field by
  the dragged distance), undo (one step per call, a group over three calls, `Undo.Undo/Redo`),
  hot reload (changed file, new `Editor` folder), the editor API (Selection, EditorPrefs file,
  AssetDB create/find/read and path checks, asset processor, EditorScene queries/components/
  parenting/destroy, game API on the edited scene, dialogs, progress, update callbacks), and the
  SampleProject scripts (load cleanly, Scatter Tool scatters 7 linked prefab copies as one undo
  step, Align to Floor, Satellite gizmos and Inspector).
  Game script gizmos also draw in projects without editor scripts (the Lua state is always made).
- `misty_engine::scripting::tests::inspector_attributes_are_read_from_the_class`,
  `asset_deps` test extended with `Editor` folders left out of builds.
