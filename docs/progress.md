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
- No transparency (Rendering Mode) and no shader selection (textures and normal maps: see below).
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
- Lua instead of C#; no coroutines, raycasts, GetComponent for scripts or other components.
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
- One scene at a time (no additive multi-scene editing).
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

## Prefabs ✅

### Done
- `.prefab` assets (same readable JSON as `.scene`, exactly one root object).
- Drag a Hierarchy row onto the Project window → `Name.prefab` in the current folder (numbered if taken); the
  object becomes an instance of it.
- Drag a `.prefab` into the Scene view (placed at the Scene view pivot) or the Hierarchy (empty space = root at the
  pivot, onto a row = child of that row) → new instance; undoable.
- Instances: blue names in the Hierarchy (root and children), blue cube icon on the root; the link is saved in
  scenes.
- Inspector "Prefab" row on instance roots: **Select** (shows the asset in the Project window and Inspector),
  **Revert** and **Apply All** (enabled when the instance differs from the asset). Apply All writes the asset and
  refreshes the other instances in the open scene.
- Hierarchy context menu ▸ Prefab ▸ Select Asset / Unpack Prefab (undoable).
- Project window: blue prefab icon; selecting a prefab shows a read-only summary (object tree, root components).
- Scripting: `Instantiate(prefabPath or gameObject [, position])` creates "Name(Clone)" with physics bodies,
  colliders and scripts (`Awake` runs right after the calling callback).

### Not matching Unity yet
- No Prefab Mode (double-click a prefab just logs a hint), no prefab variants, no nested-prefab tracking (a linked
  child keeps its own link but isn't refreshed with its asset).
- Overrides are not tracked per property: there is no Overrides dropdown, no bold override labels and no
  per-component Apply/Revert. Apply All replaces the other instances with the asset, so their own overrides are
  lost (Unity keeps them); Revert / Apply All give the instance's objects new ids.
- Instances don't follow edits made to the `.prefab` file outside the editor until reverted.
- Missing prefab assets are only shown in the Inspector ("Missing: Name"), not as red Hierarchy rows.
- Creating a prefab never overwrites an existing file (Unity asks to Replace); dragging an instance asks nothing
  about Original vs Variant.
- Objects added to an instance are blue like the rest (no "+" badge); `Instantiate` doesn't take a rotation or parent.

## Standalone player and Build Settings ✅

### Done
- `misty_player` executable (winit + wgpu): opens a window titled with the product name at the Build Settings
  size (or borderless fullscreen), plays build scene 0 with the same loop as Play mode (physics, Lua scripts,
  `Time`, keyboard/mouse input) and renders from the highest-Depth enabled camera with the editor's renderer
  (HDR, bloom, shadows). Resizing works; closing the window quits. Logs go to `Player.log` next to the exe.
- File ▸ Build Settings… (Ctrl+Shift+B): Scenes In Build list with enabled checkboxes, build indices, Add Open
  Scenes, Remove Selected, move up/down; Player section with Product Name, Resolution, Fullscreen. Saved to
  `ProjectSettings/BuildSettings.json`.
- **Build** (pick a folder) and **Build And Run** / File ▸ Build And Run (Ctrl+B, last folder): copies the player
  as `<Product>.exe` plus the project's `Assets` and `ProjectSettings`; Console messages for success, a missing
  player (`cargo build -p misty_player --release`) and other failures.
- Scripting: `SceneManager.LoadScene(name or index)`, `SceneManager.GetActiveScene().name`, `Application.Quit()`.
  In the editor LoadScene swaps the Play-mode scene until Stop; Quit logs that it is ignored.
- `MISTY_SCREENSHOT=out.png misty_player` saves the game image after 60 frames and exits.

### Not matching Unity yet
- No platform choice, compression, asset packing or stripping: the build ships the whole `Assets` folder as files.
- No splash screen, resolution dialog, icon or company name; no `Screen` API, cursor lock or VSync setting.
- No additive or async scene loading, `DontDestroyOnLoad`, or `sceneLoaded` callbacks; `GetActiveScene` has only
  `name`.
- Unsaved scene changes aren't saved before a build (a Console warning says the saved file is used).
- The editor can't build the player itself; it must exist next to the editor executable.

## Audio ✅

### Done
- **Audio Source** (Add Component ▸ Audio, or GameObject ▸ Audio ▸ Audio Source): AudioClip, Mute, Play On Awake,
  Loop, Volume, Pitch, Stereo Pan, Spatial Blend (2D ↔ 3D), 3D Sound Settings (Logarithmic / Linear rolloff, Min
  and Max Distance), enable checkbox. Clips are `.wav` / `.ogg` / `.mp3` files under `Assets/` saved by path.
- **Audio Listener** (enable checkbox); the default scene's Main Camera has one.
- Play mode: Play On Awake sources start after the scripts' `Awake` (also for objects made by `Instantiate`); each
  frame 3D sources are re-mixed from the listener and source world positions (distance rolloff + left/right pan,
  blended by Spatial Blend); disabled / inactive / destroyed sources stop; everything stops on Stop. The editor's
  Pause holds all sounds, Step doesn't resume them.
- Game view **Mute Audio** toggle silences Play-mode audio (also when toggled before Play).
- AudioClip field: dropdown of the project's audio files, or drop a clip from the Project window onto it.
- Project window: selecting a clip shows its format/length (WAV) and ▶ Play / ■ Stop preview buttons.
- Scene view: 🔊 icon (clickable) for Audio Source objects without a mesh; min/max distance spheres when a 3D
  source is selected. Hierarchy/Inspector icons.
- Scripting: `GetComponent("AudioSource")` with `Play`, `Stop`, `Pause`, `UnPause`, `PlayOneShot`, `isPlaying`,
  `volume`, `pitch`, `loop`, `mute`, `spatialBlend`, `enabled`, `clip`.
- Game view Stats shows the number of playing sounds.
- No sound device = Console warning and silent play; the device is only opened when the first sound plays.
- Sample clip `SampleProject/Assets/Audio/Beep.wav` (generated by `audio::wav::sine_beep`).

### Not matching Unity yet
- No Audio Mixer / output groups, effects (reverb, filters), Doppler, Spread, Priority, Reverb Zone Mix, custom
  rolloff curves or the rolloff graph in the Inspector; no Audio Project Settings (global volume, DSP buffer).
- Spatialisation is volume + stereo pan only (no HRTF, no front/back cue); only the first enabled listener is used
  and there's no "more than one listener" warning.
- Negative pitch plays forwards (no reverse playback); clips are fully decoded into memory (no streaming / load
  type / compression import settings); no waveform preview or auto-play in the clip Inspector.
- `PlayOneShot` takes a clip path rather than an AudioClip object; no `PlayClipAtPoint`, `time`, `PlayDelayed`,
  `PlayScheduled`, or `AudioListener.volume/pause` in scripts.
- Gizmo spheres use the source position only (Unity draws them the same way, but also shows them for 2D sources).

## Textures and 3D models ✅

### Done
- Textures: PNG/JPG under `Assets/` (sRGB albedo, linear normal maps), mipmaps generated on import (averaged in
  linear light), trilinear + anisotropic filtering, repeat wrapping. Cached by path; edited files reload in Edit
  mode. Missing files fall back to white / flat normal with a Console warning.
- Materials: Albedo texture slot (× colour), Normal Map slot + strength, Tiling and Offset. Slots are dropdowns of
  the project's textures and accept textures dragged from the Project window. Old `.mat` files load unchanged.
- Normal mapping in the Standard shader (tangent space, OpenGL/green-up convention); tangents are generated for
  primitives and imported meshes. Shadows, emission/bloom and the selection outline work as before.
- Model import: `.gltf` / `.glb` (meshes, node hierarchy with TRS, PBR metallic-roughness materials, base colour
  and normal textures, embedded GLB images) and `.obj` (objects, normals, UVs, `.mtl` diffuse/normal textures).
  Each model material is extracted once as `ModelName_Material.mat` next to the model; embedded images are saved
  as PNG beside it. Extracted materials are never overwritten, so edits stick.
- Meshes inside models are referenced as `Assets/Models/Ship.glb#0`; Mesh Filter's dropdown lists primitives plus
  every imported mesh, and a model dropped on the slot uses its first mesh. Old scenes (`"mesh": "Cube"`) load.
- Drag a model from the Project window into the Scene view (placed at the pivot) or the Hierarchy (root or child of
  a row): one object per node with Mesh Filter + Mesh Renderer using the extracted materials; undoable.
- Picking, Frame Selected, shadow bounds, wireframe and Mesh Collider (convex hull or triangle mesh) use the
  imported mesh's own bounds and triangles.
- Project window: 🖼 textures with an image preview in the grid, 📦 model files. Assets ▸ Refresh / Ctrl+R
  re-scans textures and models (new models are imported then).
- Sample project: `Textures/Checker.png`, `Textures/Bumps.png` (normal map), `Models/Ramp.obj`,
  `Materials/Checker.mat`, `Materials/Bumpy Tiles.mat` and the `Scenes/TexturesDemo.scene` showing them.

### Not matching Unity yet
- No model import settings (scale factor, normals/tangents mode, read/write, material location) and no `.fbx`.
- Models are not prefab-like assets: instances are plain objects, so later changes to the model file don't update
  their hierarchy (meshes do reload). No model preview in the Inspector; models don't expand into sub-assets in the
  Project window.
- No texture import settings (max size is 4096, wrap/filter fixed, no compression); no texture Inspector preview.
- Only Albedo and Normal Map slots: no metallic/smoothness, occlusion, height, emission or detail maps; glTF
  metallic-roughness/occlusion/emissive textures, texture transforms and vertex colours are ignored.
- No skinning, animation, blend shapes, cameras or lights from glTF.

## Animation ✅

### Done
- **Animation clips** (`.anim`, JSON in `Assets/`): length, Loop, sample rate, curves and events. A curve animates
  one property of the animated object or of a child found by name path: Transform Position / Rotation (Euler) /
  Scale x/y/z, Light Intensity / Range / Color r/g/b, GameObject Is Active. Keys have Auto (smooth, no
  overshoot), Linear or Constant tangents; evaluation is cubic Hermite. Assets ▸ Create ▸ Animation makes an
  empty clip; clips show with a ⏵ icon in the Project window.
- **Animation component** (Add Component ▸ Miscellaneous ▸ Animation): default Animation clip, Animations list
  (slots accept clips dragged from the Project window), Play Automatically, Wrap Mode (Default / Once / Loop /
  Ping Pong / Clamp Forever), enable checkbox.
- Play mode and the standalone player: Play Automatically clips start after `Awake` (also for instantiated
  objects); animations are sampled after `Update`, before `LateUpdate`; animation events call script functions
  by name. Lua: `GetComponent("Animation")` with `Play([name])`, `Stop()`, `IsPlaying(name)`, `isPlaying`,
  `time`, `speed`, `clip`, `enabled`.
- **Animation window** (Window ▸ Animation ▸ Animation, Ctrl+6; a tab next to Project/Console in every built-in
  layout): animates the selected object or its nearest parent with an Animation.
  - "To begin animating X, create an Animation Clip" + Create (→ `Assets/Animations/X.anim`, component added).
  - Toolbar: Preview, Record (red), first / previous key / Play / next key / last, frame field; clip dropdown
    with Create New Clip..., Add Keyframe (◆+), Samples.
  - Property list grouped like "Cube : Position" (foldable, with x/y/z rows), current values (editing one keys
    it at the playhead; red underline while recording), missing objects in yellow "(Missing!)", right-click Add
    Key / Remove Properties, Add Property menu listing the object's and its children's animatable properties.
  - Dopesheet: seconds:frames ruler (click/drag to scrub), summary row, diamonds per row; click / Shift+click /
    box select, drag to move (snapped to frames), double-click a row to add a key, Delete or right-click Delete
    Key, right-click tangent mode (Auto / Linear / Constant). Wheel zooms, Shift+wheel scrolls; event flags on
    the ruler; time past the clip end is shaded.
  - Curves view: the curves (all, or the rows clicked in the list) with value grid; keys can be selected, box
    selected and dragged in time and value.
  - Preview poses the scene at the playhead and restores every touched value when the preview ends (also before
    Play, Save and scene changes). Record mode turns any Inspector / gizmo change of the animated objects into
    keys (plus a key at 0 with the old value for newly recorded properties).
  - Key edits are undoable with Ctrl+Z / Ctrl+Y in the normal history; clips are saved automatically.
- Sample: `Scenes/AnimationDemo.scene` — a bobbing parent whose child cube spins (`Animations/Bob.anim`, path
  binding) and a blinking, colour-changing point light (`Animations/Blink.anim`, Constant keys);
  `Scripts/AnimationControl.lua` pauses/resumes with Space and changes speed with Up/Down.

### Not matching Unity yet
- No Animator / Animator Controller (state machine, parameters, transitions, blend trees, layers, avatar /
  humanoid retargeting, root motion); only the legacy-style Animation component (see decisions).
- Material colour and other component properties (Camera, Audio Source, script fields) can't be animated; no
  Quaternion / Euler interpolation options for rotation.
- Curves: no weighted, free or broken tangents, tangent handles, curve presets, pre/post wrap modes per curve.
- No events editing in the window (events are written in the `.anim` file); events take no parameter.
- No cross-fade/blend, `animation["Clip"]` states, `Sample()`, `Rewind()`, culling type or Animate Physics.
- Window: no ripple editing, copy/paste of keys, filter by selection, show-only-selected toggle, time/frame
  display switch, or vertical scrolling of very long property lists. Animated Inspector fields are not tinted
  (only the window's value fields). Saving while previewing leaves the preview instead of keeping it.
- The clip Inspector (a `.anim` selected in the Project window) only has Loop Time; length, sample rate, curves
  and events are shown read-only (Unity also has loop pose, cycle offset, root motion and import settings).

## Runtime UI ✅

### Done
- Components: **Canvas** (Screen Space - Overlay, Sort Order), **Canvas Scaler** (Constant Pixel Size / Scale With
  Screen Size with Reference Resolution and Match Width-Height), **Rect Transform** (anchors, pivot, anchored
  position, size delta), **Image** (colour, optional project texture, Raycast Target), **Text** (text, font size,
  line spacing, colour, 9-way alignment, Wrap/Overflow, Raycast Target) and **Button** (Interactable, Colour Tint
  Normal/Highlighted/Pressed/Disabled colours, Colour Multiplier, On Click list of target object + Lua method).
  All save/load with defaults for missing fields; On Click targets survive scene files, prefabs and duplicates.
- Layout every frame from parent rects and anchors (pure maths, unit-tested), with the Canvas Scaler factor.
- Rendering: the UI is drawn over the camera image in the Game view and in the standalone player (shared engine
  pass: coloured/textured quads + a glyph atlas from the built-in font, crisp at every scale).
- Input while playing: buttons highlight on hover, darken while pressed, fade when not interactable; a click (press
  + release on the same button) calls the On Click methods on the target's scripts. Works in the Game view (its
  pixels) and in the player window. Raycast Target off lets clicks through.
- Lua: `GetComponent("Text").text`, `Image.color`, `Button.interactable`, `Color.new(r, g, b [, a])` and more
  (see scripting.md).
- Editor: GameObject ▸ UI ▸ Canvas / Text / Image / Button (also in the Hierarchy + and right-click menus); UI
  elements go into the selected canvas or the scene's first canvas, and a Canvas is created when there is none.
  Objects get the UI layer and unique names. Component ▸ Layout / UI and Add Component list the new components.
- Inspector: Rect Transform replaces the Transform section (Pos X/Y + Width/Height, or Left/Right/Top/Bottom on
  stretched axes; Anchors Min/Max; Pivot) with an Anchor Presets popup (4×4 grid, "also set pivot / position"
  toggles or Shift/Alt); "Some values driven by Canvas." on canvases. Editors for Canvas, Canvas Scaler, Image
  (texture slot accepts drag & drop), Text (multi-line text, alignment buttons) and Button (On Click list with an
  object picker and method name).
- Scene view: canvas rectangles (one unit per screen pixel), selected UI rects with anchor markers; Frame Selected
  frames UI rects.
- Sample: `Assets/Scenes/UIDemo.scene` with a score Text and an "Add Point" Button handled by
  `Assets/Scripts/ScoreKeeper.lua` (the button disables itself and turns green at 10 points).
- Tests: layout maths, canvas layout tree, text wrapping/alignment and glyph atlas, raycasts/clicks, draw list,
  serialisation round trip, Lua API + On Click, kittest of the GameObject ▸ UI menu, and the UIDemo click-through.

### Not matching Unity yet
- Only Screen Space - Overlay: no Screen Space - Camera / World Space canvases, Pixel Perfect, or Graphic Raycaster
  settings; no EventSystem object, keyboard/gamepad navigation, Selected state or colour fade duration.
- One built-in font (Ubuntu Light); no font assets, rich text, Best Fit, vertical truncation or TextMeshPro.
- Image has no sprite types (Sliced/Tiled/Filled), Preserve Aspect or Set Native Size; textures don't hot-reload.
- No layout groups, Content Size Fitter, masks, scroll views, toggles, sliders, input fields or dropdowns.
- Rect Transform ignores the Transform's rotation/scale; no Rect tool handles to drag UI rects in the Scene view,
  and UI graphics are not drawn in the Scene view (outlines only). Clicking UI in the Scene view doesn't select it.
- On Click picks a method by name (no dropdown of the target's methods, no arguments).
- The player draws no UI while no camera renders (the screen stays black).

## Particle System ✅

### Done
- `Particle System` component (Add Component ▸ Effects, or GameObject ▸ Effects ▸ Particle System) with modules:
  Main (Duration, Looping, Prewarm, Start Delay/Lifetime/Speed/Size/Color, Gravity Modifier, Simulation Space
  Local/World, Play On Awake, Max Particles, Auto Random Seed), Emission (Rate over Time, Bursts with time, count,
  cycles, interval, probability), Shape (Sphere, Hemisphere, Cone angle/radius, Box), Color over Lifetime
  (colour + alpha gradient keys), Size over Lifetime (start → end), Renderer (billboard, optional texture, Alpha
  Blended / Additive, HDR intensity). Numbers and Start Color support Constant / Random Between Two Constants.
- Deterministic, frame-rate independent simulation in `misty_engine::particles` with unit tests (rate counts,
  bursts and cycles, lifetime death, max cap, gravity, delay, prewarm, play/stop/pause/clear, local vs world).
- Rendering: instanced camera-facing billboards after opaque meshes, depth-tested without depth writes, alpha
  systems sorted back to front; colours above 1 bloom. Works in the Scene view, Game view and `misty_player`.
- Play mode and the player run systems with Play On Awake (also on instantiated prefabs).
- Edit mode: selecting a system (or a parent of systems) previews it in the Scene view with a "Particle Effect"
  overlay: Play/Pause, Restart, Stop, Playback Speed, Playback Time, particle count. Shape gizmo (cone, sphere,
  hemisphere, box) on the selected system; ✨ icon in the Hierarchy and Scene view (clickable).
- Inspector: main section plus collapsible module sections with enable checkboxes; burst table; gradient bar with
  editable keys.
- Lua: `GetComponent("ParticleSystem")` with `Play()`, `Stop()`, `Pause()`, `Clear()`, `Emit(n)`, `isPlaying`,
  `isPaused`, `isStopped`, `particleCount`, `time`.
- Sample: `Scenes/ParticlesDemo.scene` (campfire: fire, sparks with a Space-to-burst script, smoke, fire light; `Wood.mat`, `Dirt.mat`, `SparkBurst.lua`).

### Not matching Unity yet
- Only the modules above: no Velocity/Force/Limit Velocity/Rotation/Noise/Collision/Sub Emitters/Texture Sheet
  Animation/Trails/Lights modules, no curves (only constants, random between two constants, a linear start→end
  size ramp and a gradient), no Start Rotation / 3D start size.
- Renderer: billboard only (no stretched/horizontal/vertical/mesh), no particle materials (texture + blend +
  intensity instead), no soft particles, no per-particle sorting across different systems.
- Shape: no Circle, Edge, Mesh, Donut or Rectangle shapes, no radius thickness, arc, or cone length/volume
  emission; Box always emits along +Z. Transform scale scales the shape positions but not particle size.
- Deactivating an object hides its particles but doesn't reset the system; `Stop()` takes no
  `withChildren`/`stopBehavior` arguments; no `main`/`emission` property access from Lua.
- The editor preview doesn't show the Simulate Layers / Resimulate options; Playback Time is per-play, not
  scrubbable.
