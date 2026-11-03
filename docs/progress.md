# Progress

## Step 1 — Window + editor layout ✅

### Done
- Native window (winit) rendered by wgpu through eframe; Unity-like dark theme, OS UI font + symbol fallback.
- Dockable tabs (drag, split, float, close) in the Default layout: Hierarchy | Scene+Game | Inspector,
  Project+Console along the bottom (see Window layouts below for the other layouts and saving).
- Menu bar: File, Edit, Assets, GameObject, Component, Window, Help with shortcut hints.
  Unbuilt items log a warning in the Console.
- Main toolbar: View/Move/Rotate/Scale/Rect/Transform tools (Q W E R T Y), Pivot/Center, Global/Local,
  Play/Pause/Step, Layers ▾, Layout ▾.
- Hierarchy: scene root row with `*` dirty marker, create `+` menu, search filter, select, right-click menu.
- Inspector: active toggle, name, Static, Tag/Layer, Transform foldout with draggable X/Y/Z labels, Add Component.
- Project: real `Assets` folder tree, breadcrumb, grid/list view (icon size slider), search, refresh (Ctrl+R).
- Console: Clear, Clear on Play, Collapse, Error Pause, info/warning/error toggle counters, search, detail pane.
- Game view: Display, aspect ratios with letterboxing, Scale slider, Play Focused ▾, Mute, Stats, Gizmos.
- Status bar shows the latest Console message; clicking it focuses the Console.
- Shortcuts: Ctrl+P / Ctrl+Shift+P / Ctrl+Alt+P, Ctrl+1..5, Ctrl+Shift+C, Ctrl+N, Ctrl+Shift+N, Ctrl+D, Del, Shift+D.

### Not matching Unity yet
- No tab ⋮ menu (Maximize / Add Tab), no double-click-to-maximize.
- Cut/Copy/Paste not implemented.

## Step 2 — Scene view with camera movement ✅

### Done
- Real 3D rendering (wgpu, 4x MSAA): procedural sky gradient, infinite grid with 1/10/100 LOD and distance fade.
- Navigation: RMB look + WASD/QE fly (Shift = 4x, speed ramps up, RMB+wheel changes speed), MMB pan,
  View tool (Q) + LMB pan, Alt+LMB orbit, Alt+RMB zoom, wheel zoom, F / double-click frames the selection.
- Scene gizmo (top-right): click an axis cone to snap to Right/Left/Top/Bottom/Front/Back (orthographic), click the
  centre or the label to toggle Persp/Iso; label shows "< Persp", "< Iso" or the axis name. Smooth view animation.
- 2D mode: orthographic, looking down +Z, RMB pans, XY grid, gizmo hidden; leaving 2D restores the 3D view.
- Unity 6 style: vertical tools overlay in the view; Pivot/Center and Global/Local dropdowns, grid toggle, draw mode,
  2D, lighting, audio, skybox, camera settings (FOV, dynamic clipping, near/far, speed), Gizmos, search.
- Gizmos: camera/light icons, camera frustum and light rays when selected, wire shapes for created primitives
  (selected = orange). Click icons to select, click empty space to deselect.
- Game view renders from the scene's Main Camera (skybox); "No cameras rendering" when there is none.
- New objects spawn at the Scene view pivot.
- 18 automated tests (camera maths, shader validation, headless scene-view input).

### Not matching Unity yet
- No rectangle (marquee) selection; picking uses icon/centre distance, not mesh raycasts.
- No Persp↔Iso blend animation (it switches instantly); the camera does not slide when the projection changes.
- Scene-view search and the audio toggle have no effect yet.
- No "Move to View" / "Align With View".
- Icons do not scale with distance (Unity's 3D icons option).

## Step 3 — GameObjects, Transform, Hierarchy ✅

### Done
- Engine `scene` module on hecs: GameObject/Transform/Node + Camera, Light, Mesh Filter, Mesh Renderer components,
  component registry, hierarchy ops (re-parent with keep-world, reorder, deep duplicate, recursive delete), world
  matrices, active-in-hierarchy, Unity-style "(1)" naming.
- Hierarchy: tree with foldouts (Alt+click = whole subtree), click / Ctrl / Shift multi-select, drag to re-parent or
  reorder (blue line / box drop indicator, drag label), drop on empty space = root, inline rename (F2, Enter, Esc),
  arrow keys (Up/Down select, Left/Right fold), search (flat results), inactive objects greyed, grey selection when
  the panel is unfocused, scene ⋮ menu, per-row context menu (Rename, Duplicate, Delete, Select Children, Create …
  as child).
- GameObject menu: Create Empty / Empty Child / Empty Parent, 3D Objects, Lights, Camera, Clear Parent, sibling
  order, Move To View, Align With View, Align View to Selected, Toggle Active State. Edit: Undo/Redo (with action
  names), Select All, Duplicate, Rename, Delete, Frame Selected.
- Undo/Redo for every command and Inspector edit (Ctrl+Z, Ctrl+Y / Ctrl+Shift+Z).
- Scene view: picking by ray vs mesh bounds and camera/light icons; Ctrl/Shift-click multi-select; selected meshes
  orange; frame selection fits all selected bounds. Game view renders from the highest-Depth enabled camera with its
  FOV/ortho/clipping and clear flags.

### Not matching Unity yet
- No Copy/Paste of objects; no Hierarchy auto-scroll while dragging; no "pinging" of selected objects.
- Hierarchy does not scroll to the selection when it is picked in the Scene view (it does expand the parents).

## Step 4 — Inspector (edit values live) ✅

### Done
- Inspector header: icon, active, name, Static, Tag, Layer. Foldout per component with enable checkbox and ⋮ menu
  (Reset, Remove Component, Move Up/Down, Copy Component, Paste Component Values).
- Editors: Transform (draggable X/Y/Z labels, Constrain Proportions lock), Camera (Clear Flags, Background,
  Projection, FOV/Size, Clipping Planes, Viewport Rect, Depth), Light (Type, Range, Spot Angle, Color, Intensity,
  Shadow Type), Mesh Filter (Mesh), Mesh Renderer (Color, Cast/Receive Shadows).
- Add Component button with search popup grouped by category; Component menu lists addable components; Ctrl+Shift+A.
- Multi-object editing (shared components only), Inspector lock.
- Every edit is live in the Scene and Game views and undoable as one step per drag/typing session.
- Scene-view Move / Rotate / Scale / Transform handles: hover highlight (yellow), axis / plane / free move, axis and
  view-ring rotation, axis and uniform scale, Pivot/Center and Global/Local, Ctrl snapping, works on multi-selection;
  pressing a handle never changes the selection.
- 42 automated tests, including headless drags of the move/rotate/scale handles and the Add Component popup.

### Not matching Unity yet
- Mixed values ("—") are not shown when multi-selected objects differ; the active object's values are displayed.
- Rect tool has no handles; no vertex snapping (V), no Shift+drag screen-space move, no surface snapping.
- No Debug Inspector mode, no component help (?) icon, no presets.

## Step 5 — Meshes, materials, lights ✅

### Done
- Primitive meshes at the familiar sizes: Cube, Sphere, Capsule, Cylinder, Plane (10×10), Quad.
- Lit rendering (4x MSAA): directional / point / spot lights with range falloff and spot cone, colour + intensity,
  hemisphere ambient, GGX specular + sky reflections (Smoothness / Metallic visible), shadows for directional,
  spot and point lights with Shadow Type Hard (crisp) / Soft (filtered) / None.
- Orange selection outline around selected objects (children included).
- Scene view draw modes: Shaded, Wireframe, Shaded Wireframe. Lighting toggle off = headlight from the camera.
- Material assets: Project ▸ + ▸ Material / Assets ▸ Create ▸ Material, Folder; select a material in the Project
  window to edit it in the Inspector (Albedo, Metallic, Smoothness, Emission); changes save to disk and show live.
- Mesh Renderer: Materials ▸ Element 0 picker; inline material editor under the components; drag a material from
  the Project window onto an object in the Scene view, a Hierarchy row, or the Element 0 slot.
- Game view renders the same lit scene from the game camera.
- Sample materials: `Assets/Materials/Red.mat`, `Blue Metal.mat`.

### Not matching Unity yet
- No textures, normal maps or transparency (Rendering Mode); no shader selection.
- No lightmapping, reflection probes or fog; local-light shadows are limited to 16 layers (e.g. 2 point + 4 spot).
- Material changes are not undoable; assets can't be renamed/deleted from the Project window yet.
- No material preview sphere in the Inspector; no drag-hover preview in the Scene view.

## Step 6 — Play / Pause / Stop ✅

### Done
- Play (Ctrl+P / ▶): snapshot of scene + selection + undo history; editor tint (blue tab strip, highlighted ▶);
  focuses the Game view (Play Focused), maximises it (Play Maximized) or leaves the layout (Play Unfocused).
- Stop: everything changed during Play is reverted; edit-mode undo history comes back.
- Pause (Ctrl+Shift+P): freezes time; can be armed before Play to start paused. Step (Ctrl+Alt+P): one frame.
- Console: Clear on Play, Error Pause.
- Game view Stats overlay: FPS / frame ms, render time, batches, tris, verts, screen size, play time, frame.
- Game loop with Time (time, delta, frame, timeScale) ready for physics and scripts.

### Not matching Unity yet
- No whole-editor colour tint (only the tab strip turns blue); Inspector edits during Play are not highlighted.
- No "Enter Play Mode Options", no Game-view input capture yet (arrives with scripting input).

## Step 7 — Physics ✅

### Done
- Components (Component ▸ Physics / Add Component): Rigidbody (Mass, Drag, Angular Drag, Use Gravity, Is Kinematic,
  Collision Detection Discrete/Continuous, Constraints Freeze Position/Rotation XYZ), Box / Sphere / Capsule (X/Y/Z
  direction) / Mesh (Convex) colliders with Is Trigger, Friction, Bounciness, Center, Size/Radius/Height.
- 3D Objects are created with their matching collider.
- Play mode simulates: gravity, collisions between dynamic/static/kinematic bodies, compound colliders on parents,
  collider sizes follow object scale; Stop restores the pre-Play scene.
- Green collider gizmos for selected objects.

### Not matching Unity yet
- No joints, no physics materials as assets, no layer collision matrix.
- No "Edit Collider" handles; Mesh Collider only works with built-in meshes.

## Step 8 — Scripting ✅

### Done
- Lua scripts as assets: Project ▸ + ▸ Script, Add Component ▸ Scripts / New script, drag onto objects in the Scene
  view, Hierarchy, or Inspector; double-click to edit in the default editor; hot reload of fields in Edit mode.
- Inspector "Name (Script)" sections: enable toggle, Script field, public fields (number, bool, string, Vector3)
  with readable labels; ⋮ Reset / Remove Component / Edit Script; several scripts per object.
- Lifecycle: Awake, Start, Update, FixedUpdate, LateUpdate, OnCollisionEnter/Exit, OnTriggerEnter/Exit.
- API: GameObject, Transform, Rigidbody, Vector3, Time, Input (keys, mouse, axes), Debug.Log/Warning/Error, print,
  GameObject.Find, Destroy, Mathf, ForceMode (see docs/scripting.md).
- Script logs and errors (with script name, callback and line) go to the Console; Error Pause works.
- Game view gets keyboard/mouse while focused; Stats show bodies, colliders and script count.
- Samples: Rotator.lua, PlayerController.lua.

### Not matching Unity yet
- Lua instead of C#; no coroutines, Instantiate/prefabs, raycasts, GetComponent for scripts or other components.
- No script execution order settings; no "Edit Script" opening at the error line.
- Hot reload only applies in Edit mode.

## Step 9 — Save and load scenes ✅

### Done
- `.scene` files (readable JSON): every object, hierarchy, component and Inspector value, material/script references,
  script field overrides; stable output for version control; tolerant loading with Console warnings.
- File ▸ New Scene (Ctrl+N), Open Scene (Ctrl+O, native dialog), Open Recent Scene, Save (Ctrl+S), Save As
  (Ctrl+Shift+S); Hierarchy scene ⋮ ▸ Save Scene / Save Scene As.
- Project window: + ▸ Scene creates a scene asset; double-click a scene to open it.
- "Scene Has Been Modified" prompt before New / Open / quit (window close included); `*` in the Hierarchy and the
  window title while unsaved; reopens the last scene on startup.
- Sample project ships `Assets/Scenes/SampleScene.scene` (spinning cube, rollable player, crate stack, lights).

### Not matching Unity yet
- One scene at a time (no additive multi-scene editing); no prefabs; no Build Settings scene list.
- Asset references are paths, so moving/renaming an asset outside the editor breaks references (no GUIDs/.meta yet).
- Window layout isn't persisted.

## Project Settings ✅

### Done
- Edit ▸ Project Settings… opens a floating "Project Settings" window: category list (Physics, Time) on the left,
  fields on the right. Every change is saved immediately to `<Project>/ProjectSettings/ProjectSettings.json`
  (meant for version control) and loaded when the editor opens the project.
- Physics: Gravity, Max Steps Per Frame. Time: Fixed Timestep, Maximum Allowed Timestep, Time Scale.
- Play mode builds physics with the project's gravity / fixed timestep / step cap; Time Scale and Maximum Allowed
  Timestep also apply live while playing. Lua `Time.fixedDeltaTime` and `Time.timeScale` show the project values.
- Missing or unknown fields load as defaults; out-of-range values are clamped.

### Not matching Unity yet
- Only Physics and Time categories (no Player, Quality, Input, Tags and Layers, Graphics, Audio…).
- No search field, no Reset in a ⋮ menu, no per-setting undo; scripts can't change `Time.timeScale` yet.
- No default physics material: friction and bounciness are per collider.


## Window layouts ✅

### Done
- The dock arrangement (including floating windows) is saved per user in `<Project>/Library/Layouts.json`
  whenever it changes (at most once a second) and on exit, and restored on startup; a missing or corrupt file
  falls back to the Default layout. During Play Maximized the pre-Play layout is the one saved.
- Window ▸ Layouts and the toolbar Layout ▾ (which shows the current layout's name): built-in **2 by 3**,
  **4 Split**, **Default**, **Tall**, **Wide**; saved user layouts; **Save Layout...** (name prompt, replaces a
  layout with the same name); **Delete Layout ▸**; **Reset All Layouts** (deletes user layouts, back to Default).
- The main window's size, position and maximized state are remembered between runs (eframe persistence).

### Not matching Unity yet
- Built-in layouts can't be deleted; user layouts are per project rather than per machine.
- 4 Split has one Scene view (multiple Scene views aren't supported yet), so it shows Scene, Game, Hierarchy and
  Project in the grid.
