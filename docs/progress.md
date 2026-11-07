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
- Rigidbody ▸ Interpolate (None / Interpolate) blends the drawn pose between fixed steps.
- Layer Collision Matrix in Project Settings ▸ Physics (unchecked pairs don't collide, trigger or block
  controllers) and **Queries Hit Triggers**.
- Queries: `Physics.Raycast`, `RaycastAll`, `SphereCast`, `OverlapSphere` with layer masks (default skips Ignore
  Raycast; rays ignore colliders they start in); `LayerMask.GetMask/NameToLayer/LayerToName`; work from `Start`.
- Joints (Component ▸ Physics, a Rigidbody is added with them): **Fixed Joint**, **Hinge Joint** (anchor, axis,
  auto-configured connected anchor, spring, motor, limits), **Spring Joint** (spring, damper, min/max distance),
  each with Connected Body (another Rigidbody, or the world) and Enable Collision; Inspector editors with a
  Connected Body picker; Scene-view gizmos (anchor cross, hinge axis arrow and limit arc, spring zigzag).
- **Character Controller** (slope limit, step offset, skin width, min move distance, center, radius, height):
  kinematic capsule driven by rapier's character controller; Lua `Move`, `SimpleMove`, `isGrounded`, `velocity`,
  `OnControllerColliderHit`; capsule gizmo. Dynamic bodies collide with it.
- Lua Rigidbody extras: `MovePosition`, `MoveRotation`, `position`, `isKinematic`, `useGravity`; `gameObject.layer`.
- Sample: `Scenes/PhysicsDemo.scene` — capsule character (W/S walk, A/D turn, Space jump, camera follows) with a
  raycast laser pointer (click/E shoves what it points at), a self-closing hinged door it can push open, a
  motorised turnstile, a spring chain with a weight, steps, a ramp and crates (`CharacterWalker.lua`,
  `LaserPointer.lua`, `Laser.mat`).

### Not matching Unity yet
- No physics materials as assets, no Tags & Layers editor (six fixed layers), no `Physics.gravity` from Lua,
  no `BoxCast`/`CapsuleCast`/`OverlapBox`, no `QueryTriggerInteraction` argument, no Collider userdata
  (`hit.collider` is the GameObject).
- Joints: no Configurable/Character joints, no break force/torque or `OnJointBreak`, no Lua access to joints,
  no Free Spin / bounciness of limits; joint frames are fixed when Play starts. Spring Joint has no Tolerance.
- Character Controller: capsule follows the object's rotation (not kept upright), `Move` returns nothing
  (no CollisionFlags), `isGrounded` also true on flat ground without gravity, no `detectCollisions`/
  `enableOverlapRecovery`, can't be disabled; `OnControllerColliderHit` runs after the calling callback.
- No Extrapolate interpolation, no Continuous Dynamic / Speculative modes; `MoveRotation` takes Euler angles.
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
- Lua instead of C#; no coroutines, GetComponent for scripts or for every component (Physics queries arrived with the physics extension).
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
- Physics: Gravity, Max Steps Per Frame, Queries Hit Triggers, Layer Collision Matrix. Time: Fixed Timestep,
  Maximum Allowed Timestep, Time Scale.
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
- No custom rolloff curves (only Logarithmic / Linear; the Inspector graph shows Volume and Spatial Blend only),
  no Audio Project Settings (global volume, Doppler factor, DSP buffer, voice counts — 32 real voices is fixed).
- Spatialisation is volume + stereo pan only (no HRTF, no front/back cue); only the first enabled listener is used
  and there's no "more than one listener" warning. Bypass Listener Effects does nothing (filters on the listener
  aren't supported).
- Negative pitch plays forwards (no reverse playback); clips are fully decoded into memory (no streaming / load
  type / compression import settings); no waveform preview or auto-play in the clip Inspector.
- `PlayOneShot` / `PlayClipAtPoint` take a clip path rather than an AudioClip object; no `PlayDelayed`,
  `PlayScheduled`, `timeSamples`, or `AudioListener.volume/pause` in scripts.

## Audio Mixer ✅

### Done
- **Audio Mixer assets** (`.mixer` JSON; Project ▸ Create ▸ Audio Mixer): a group tree under Master; each group has
  a volume fader (dB), pitch, Mute / Solo / Bypass Effects and an effect chain — Lowpass, Highpass, Echo, Reverb,
  Distortion, Compressor (kira's built-in effects). Snapshots store every volume, pitch and effect value; one is
  the start snapshot. Values can be exposed to scripts under a name.
- Play mode: groups become kira tracks (a voice plays on its Output group's track, which feeds its parent up to
  Master); group pitch multiplies into every voice routed there; solo silences voices of groups outside the
  soloed subtrees; snapshots blend linearly with `TransitionTo(time)`; `SetFloat` overrides a value until
  `ClearFloat`.
- **Audio Mixer window** (Window ▸ Audio ▸ Audio Mixer, or double-click a mixer): Mixers, Snapshots (★ start
  snapshot; right-click: set as start, rename, duplicate, delete), Groups tree (+ adds under the selected group;
  rename, remove) and Exposed Parameters (rename, unexpose). One channel strip per group: dB readout, vertical
  fader with a dB scale (double-click = 0 dB), live peak VU meter while playing, M / S / B buttons, the
  Attenuation slot, effect slots (click to select; right-click: bypass, move, remove) and Add....
- Inspector: a selected group shows its name, Pitch, Volume and each effect's parameters for the edited snapshot;
  right-click a value to expose it. The mixer asset shows a summary and an Open button. Edits save immediately and
  undo; in Play mode they apply to the running game at once (Edit in Play Mode), and the faders follow
  transitions.
- **Audio Source** Inspector in the familiar layout: AudioClip, Output (any group of any project mixer, or drop a
  mixer for its Master), Mute, Bypass Effects / Listener Effects / Reverb Zones, Play On Awake, Loop, Priority,
  Volume, Pitch, Stereo Pan, Spatial Blend, Reverb Zone Mix, and 3D Sound Settings (Doppler Level, Spread, Volume
  Rolloff, Min / Max Distance, rolloff graph with the listener's distance marked).
- Doppler (pitch from source and listener velocity along the line between them, × Doppler Level × Spatial Blend),
  Spread (narrows the 3D pan; 180° = centred, 360° = mirrored), Priority (beyond 32 voices the least important /
  quietest are silenced).
- **Audio Reverb Zone** (Min / Max Distance, preset Generic / Room / Bathroom / Hallway / Cave / Arena) with gizmo
  spheres; **Audio Low Pass / High Pass / Echo Filter** components act on the Audio Source of the same object.
- Sample `Assets/Scenes/AudioDemo.scene` (generated by `sample_audio.rs` with its mixer and WAV loops): music on
  Music, an orbiting humming sphere on SFX, keys 1/2/3 for the Normal / Muffled / Cave snapshots, M and Up/Down for
  exposed parameters, Space for a one-shot, and a UI Text hint.

### Not matching Unity yet
- No Sends / Receives / Duck Volume, no SFX Reverb's full parameter set (our Reverb has room size, damping, width,
  wet mix), no Chorus / Flange / Pitch Shifter / ParamEQ / Normalize effects, no per-parameter snapshot transition
  curves, no `TransitionToSnapshots` weights, no Views, no group colours or renaming from the strip header.
- Adding / removing / reordering groups or effects while playing rebuilds the mixer's tracks: sources routed there
  restart from the beginning and one-shots stop. The Echo delay time is fixed when a track is built.
- A mixer can't route into another mixer's group (no nested mixers).
- Reverb zones don't blend (the strongest zone at the listener wins) and only have presets; the filters have no
  distance-based cutoff curves; Audio Distortion / Chorus / Reverb Filter components don't exist.
- VU meters show the peak level per frame (no RMS, no clip hold); the null backend estimates levels from voice
  volumes for tests.

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
- Animator: see the Animator section below (no layers, avatar / humanoid retargeting or root motion).
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

## Animator ✅

### Done
- **Animator Controller** asset (`.controller`, JSON; Project ▸ Create ▸ Animator Controller, ⇄ icon):
  parameters (Float, Int, Bool, Trigger), one base layer state machine with states (clip or 1D blend tree,
  speed, loop), a default state, Entry, Any State and Exit, and transitions (Has Exit Time + Exit Time, Fixed
  Duration + Duration, Offset, conditions Greater / Less / Equals / NotEqual / If / IfNot).
- **Animator component** (Add Component ▸ Miscellaneous ▸ Animator): Controller (picker or drag a controller),
  Speed, Keep State On Disable, Open button. Evaluated in Play mode and the standalone player after `Update`
  (after the Animation component): default state on the first frame, Any State transitions first, triggers
  consumed by the transition that uses them, exit times (repeating each loop below 1), cross-fades that blend
  the sampled values, rest values restored for properties no longer animated, clip events.
- **1D Blend Trees**: a Float parameter mixes neighbouring child clips by threshold.
- **Lua**: `GetComponent("Animator")` with `SetFloat/GetFloat`, `SetInteger/GetInteger`, `SetBool/GetBool`,
  `SetTrigger/ResetTrigger`, `Play`, `CrossFade`, `CrossFadeInFixedTime`, `GetCurrentAnimatorStateInfo(0)` /
  `GetNextAnimatorStateInfo(0)` (`IsName`, `normalizedTime`, `length`, `speed`, `loop`), `IsInTransition(0)`,
  `speed`, `enabled`.
- **Animator window** (Window ▸ Animation ▸ Animator, or double-click a `.controller`; docks with the Scene and
  Game views): follows the selected object's Animator.
  - Graph with grid, wheel zoom, middle-mouse / Alt+drag pan; state nodes (default state orange), Any State
    (teal), Entry (green, arrow to the default state), Exit (red); transition arrows; drag nodes to move them.
  - Right-click: Create State ▸ Empty / From New Blend Tree; on a state Make Transition (then click the target),
    Set as Layer Default State, Delete; on a transition Delete. Delete key removes the selection. Dropping a
    clip from the Project window creates a state for it.
  - Parameters panel: + ▸ Float / Int / Bool / Trigger, double-click or right-click ▸ Rename, Delete, default
    values.
  - Selecting a state shows it in the Inspector (name, Motion Type Clip / 1D Blend Tree, Motion, Speed, Loop, the
    blend tree's parameter and motions, its transitions); a transition shows Has Exit Time, Exit Time, Fixed
    Duration, Transition Duration, Offset and the Conditions list (+ / −).
  - Play mode: the current state is outlined in blue with a progress bar (the cross-fade target too), the
    transition being taken turns blue, parameters show (and can change) the live values.
  - Every edit is undoable (Ctrl+Z) and saved to the asset immediately.
- Sample: `Scenes/AnimatorDemo.scene` — `Animations/Hero.controller` (Locomotion blend tree of `HeroIdle` /
  `HeroWalk` on Speed, Jump on a trigger from Any State, Dance on a bool) and `Scripts/AnimatorControl.lua`:
  Space jumps, 1 / 2 / 3 set the speed, 4 toggles dancing; a UI Text shows the keys and the current state.

### Not matching Unity yet
- One layer only: no layers / weights / avatar masks, sub-state machines, conditional Entry transitions, state
  machine behaviours, 2D or direct blend trees, mirror, cycle offset, speed / time parameters, root motion,
  humanoid avatars, IK, culling or update modes (Animate Physics, Unscaled Time).
- Transitions can't be interrupted, can't be muted / soloed, Any State never transitions to itself, and several
  transitions between the same two states draw as one arrow. No transition preview timeline in the Inspector.
- Loop is set per state rather than taken from the clip; Write Defaults is always on.
- Window: no marquee or multi-selection, copy / paste, Frame All, Layers tab, Auto Live Link toggle, or
  Animator preview of the selected state in edit mode; parameters can't be reordered.
- Lua: names only (no `StringToHash` / hash overloads), no `GetLayerWeight`, `runtimeAnimatorController`,
  `applyRootMotion` or `keepAnimatorStateOnDisable` from scripts.

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
- **Widgets:** Toggle (+ Toggle Group, Allow Switch Off), Slider (4 directions, min/max, whole numbers, driven
  Fill + Handle), Scrollbar (value, size, steps; click pages), Input Field (single line, caret + blink,
  selection, placeholder, character limit, Standard / Integer / Decimal / Password, read only, On Value Changed /
  On End Edit), Dropdown (options, list built from its Template, opens above when there's no room), Scroll Rect
  (drag, wheel, inertia, Elastic / Clamped / Unrestricted, linked scrollbars), Mask / Rect Mask 2D (clipping of
  drawing and clicks), Raw Image (texture + UV rect), Panel. GameObject ▸ UI creates them with the familiar child
  hierarchies (`ui::presets`); references between parts survive files, prefabs and duplicates.
- **Auto layout:** Horizontal / Vertical Layout Group (padding, spacing, child alignment, reverse arrangement,
  control child size, child force expand), Grid Layout Group (cell size, spacing, start corner/axis, alignment,
  flexible / fixed column / fixed row constraint), Content Size Fitter (min / preferred) and Layout Element
  (ignore layout, min / preferred / flexible); Text reports its preferred size.
- **Events:** an Event System object (created with the first Canvas; First Selected, navigation, drag threshold);
  hover / press / click / drag routed to widgets, selection (Selected colour), arrow-key navigation and Return /
  Space submit, Colour Tint with Normal / Highlighted / Pressed / Selected / Disabled on every selectable.
- **Lua:** `onClick` / `onValueChanged` / `onEndEdit` with `AddListener`, `toggle.isOn`,
  `slider.value/minValue/maxValue/wholeNumbers`, `scrollbar.value/size`, `inputField.text/contentType`,
  `dropdown.value/options/AddOptions/ClearOptions`, Toggle Group and Scroll Rect basics; Inspector On Value Changed
  lists pass the value to the method.
- Inspector editors for every new component (object pickers for parts, call lists, options list, optional Layout
  Element sizes), "Some values driven by …" on Rect Transforms placed by a layout group or widget.
- Sample: `Assets/Scenes/UIWidgetsDemo.scene` — a settings panel (volume slider, fullscreen toggle, name field,
  quality dropdown, scroll list of level buttons in a Vertical Layout Group) with `Assets/Scripts/SettingsMenu.lua`
  showing the choices in a Text.
- Tests: layout maths, canvas layout tree, text wrapping/alignment and glyph atlas, raycasts/clicks, draw list,
  serialisation round trip, Lua API + On Click, kittest of the GameObject ▸ UI menu, and the UIDemo click-through;
  layout groups / fitter / grid, slider from the pointer, input field editing and content types, toggle groups,
  dropdown list, scroll rect drag/clamp/wheel, mask clipping, navigation, widget Lua API + listeners, duplicate
  remapping, and the UIWidgetsDemo play-through.

### Not matching Unity yet
- Only Screen Space - Overlay: no Screen Space - Camera / World Space canvases, Pixel Perfect, or Graphic Raycaster
  settings; no gamepad input, explicit navigation modes, Tab navigation or colour fade duration.
- One built-in font (Ubuntu Light); no font assets, rich text, Best Fit, vertical truncation or TextMeshPro.
- Image has no sprite types (Sliced/Tiled/Filled), Preserve Aspect or Set Native Size; textures don't hot-reload.
- Masks are rectangles (no sprite-shaped stencil masks); Input Field is single line without clipboard shortcuts
  or rich text; Dropdown lists have no fade, item images or scroll-to-selected; Scroll Rect has no scrollbar
  visibility modes or onValueChanged; no Sprite Swap / Animation transitions, Aspect Ratio Fitter, Layout
  priorities or Canvas Group; value-changed events from scripts arrive in the next frame's UI step.
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

## Editor workflows ✅

### Done
- **Snapping**: hold Ctrl while moving / rotating / scaling with the handles to snap by increments; Edit ▸ Grid and
  Snap window (grid size, move increment per axis, rotate, scale, Grid Snapping, Align Selected to Grid, Reset),
  saved per project in `Library/SnapSettings.json`. Scene toolbar 🧲 toggle snaps moves to the world grid (Global
  handle). Ctrl+Shift on the centre move handle snaps onto surfaces; hold V for vertex-to-vertex snapping.
- **Component context menu** (⋮ or right-click on the title): Reset, Remove Component, Move Up / Move Down,
  Copy Component, Paste Component As New, Paste Component Values; all undoable, Reset/Remove/Paste act on every
  selected object.
- **Multi-object editing**: shared components only, edits apply to all, differing values show "—" (numbers,
  combos) or an indeterminate checkbox: Transform per axis, GameObject header, enable toggles, Camera, Light,
  Mesh Renderer, Audio Source, Rigidbody, colliders.
- **Project window**: right-click ▸ Create ▸ Folder / Material / Lua Script / Scene / Animation (new assets start
  renaming), Rename (F2, inline), Duplicate (Ctrl+D), Delete (to the Recycle Bin / Trash after a confirm dialog),
  Show in Explorer, Copy Path, Refresh; drag assets or folders onto a folder (tree, tile or breadcrumb) to move
  them. Renames/moves update references in every `.scene`, `.prefab`, `.mat`, `BuildSettings.json` and in the
  editor's memory (open scene, undo, Build Settings, recent scenes). Assets ▸ Show in Explorer / Rename /
  Duplicate / Delete. Search filters `t:Material t:Script t:Scene t:Prefab t:Texture t:Model t:AudioClip`
  (also `t:AnimationClip`, `t:Folder`).
- **Hierarchy / Edit**: Copy / Cut / Paste of GameObjects (Ctrl+C / X / V, Edit menu and row menu, deep, pasted
  as sibling after the selection; Paste As Child), Shift+click range select, F2 rename, Alt+click expand all.
- **Scene view**: Edit ▸ Frame Selected, Align With View (Ctrl+Shift+F), Lock View to Selected (Shift+F).
- Tests: snap maths and settings file, Ctrl/grid snapping drags, Align to Grid, lock view, component ops, paste
  values through the menu, mixed-value display, file operations and reference rewriting on a temp copy of the
  sample project, Project menu create/delete, Hierarchy copy/paste, range select and expand all.

### Not matching Unity yet
- Surface snapping uses bounds boxes and doesn't align rotation; no Push to Grid per axis, no snap for the Rect
  tool, grid snapping doesn't affect rotate/scale.
- Mixed "—" isn't shown for colours, Particle System, UI components, Animation or script fields (edits still
  apply to all). Only one component of each type per object, so Paste As New skips objects that already have it.
- Project window: single selection, no Ctrl+C/V of assets, no Favorites, references are path-based (Lua source
  strings aren't rewritten); Delete can't be undone in the editor (restore from the Recycle Bin).
- Pasted objects get numbered names ("Rig (1)"); no Paste As Child shortcut.

## Lighting window, environment and post-processing ✅

### Done
- Per-scene lighting settings saved in the `.scene` file (`"lighting"`, omitted at defaults; older scenes load
  with defaults that render exactly as before).
- Window ▸ Rendering ▸ Lighting opens a dockable Lighting tab (next to the Inspector) with Scene and Environment
  pages. Environment: Skybox (Procedural: sun disc None/Simple/High Quality, sun size, atmosphere thickness, sky
  tint, ground, exposure — or Solid Color), Sun Source (brightest Directional Light), Environment Lighting
  (Skybox / Gradient / Color + intensity multiplier), Environment Reflections intensity, Fog (colour,
  Linear/Exponential/Exponential Squared, density or start/end). Edits are undoable and mark the scene dirty.
- Procedural sky follows the main Directional Light: visible sun disc (blooms), warm horizon and darker zenith at
  sunset, dark night sky; skybox ambient is derived from the sky.
- Fog on lit meshes, alpha and additive particles, and the sky horizon.
- `Post-process Volume` component (Add Component ▸ Rendering; global, highest priority wins) with override
  sections: Tonemapping (None / Neutral / ACES), Bloom (threshold, intensity, scatter, tint), Color Adjustments
  (post exposure, contrast, colour filter, saturation), Vignette (colour, intensity, smoothness). Without a volume
  the baseline (bloom 0.6, no grading) keeps existing scenes identical.
- Applied in the Game view, `misty_player` and the Scene view; the Scene view ☁ Effects toggle (with a ⏷ menu for
  Skybox / Fog / Post Processing) turns them off.
- Lua: `RenderSettings.fog`, `fogColor`, `fogDensity`, `fogStartDistance`, `fogEndDistance`, `ambientIntensity`,
  `reflectionIntensity`.
- Sample: `Scenes/LightingDemo.scene` (sunset sky, fog, ACES + vignette; `Sand.mat`, `Sandstone.mat`).
- Tests: shader validation, CPU mirrors of the sky/ambient and tonemap/grading math, fog formulas, settings round
  trip and old-file defaults, Lighting window (menu, fog edit + undo) and tab placement, Lua RenderSettings.

### Not matching Unity yet
- No skybox materials/cubemaps (the skybox is set in the Lighting window), no Sun Source picker, no realtime
  shadow colour, no reflection probes or custom reflection cubemaps (reflections always use the sky), no
  lightmapping / Generate Lighting, no Halo/Flare settings.
- The procedural sky is our own approximation (no physical scattering); exposure default is 1, not 1.3.
- Post-processing: global volumes only (no local/box volumes, weights, blending or profile assets); no other
  effects (depth of field, motion blur, white balance, lift/gamma/gain, channel mixer, film grain, lens
  distortion…); tonemapping None compresses highlights instead of clipping; Neutral/ACES are fitted curves.
- Fog does not affect UI or gizmos (by design) and the skybox only near the horizon.

## Terrain ✅

### Done
- `Terrain` component + `.terrain` data asset (heightmap resolution 33–1025, width/length/height, 16-bit heights,
  splat map 16–512, up to 4 layers with texture, optional normal map, tile size/offset, metallic, smoothness).
- GameObject ▸ 3D Object ▸ Terrain creates an object with Terrain + Terrain Collider and a new
  `Assets/New Terrain.terrain`; the Project window shows `.terrain` files with a ⛰ icon.
- Rendering: one mesh per terrain asset (rebuilt when edited), smooth normals from the heightmap, splat-blended
  layers with normal maps, lit like other meshes (all light types, shadows cast and received, ambient,
  reflections, fog) in the Scene view, Game view and player.
- Physics: `Terrain Collider` (friction, bounciness) = static rapier heightfield; rigidbodies, raycasts and the
  Character Controller collide with it.
- Terrain Inspector with tool tabs: Raise or Lower (Shift lowers), Set Height (+ Flatten All), Smooth Height,
  Paint Texture (layer list, add/remove, per-layer settings) and Settings (Terrain Data, Cast Shadows, size,
  heightmap and control texture resolution). Brush Size / Opacity / Falloff, brush circle following the terrain
  under the mouse, painting with the left mouse button; one undo step per stroke or settings change. Clicking a
  terrain selects it; surface snapping lands on terrains.
- Lua: `Terrain.activeTerrain`, `Terrain.activeTerrains`, `GetComponent("Terrain")` with `SampleHeight`,
  `GetPosition`, `terrainData`.
- Sample: `Scenes/TerrainDemo.scene` (hills, grass/rock/sand layers from generated textures, rolling balls and
  crates, `TerrainHover.lua` marker following the ground).
- Tests: height sampling and triangle split, normals/winding, file round trip and sanitising, base64, brush
  weight/raise/lower/set height/smooth/paint, resampling, ray casts, scene helpers and versions, rigidbody and
  Character Controller on the collider, raycast hits, Lua API, editor create + undo/redo of a settings edit,
  sample generator check and a headless play of the demo.

### Not matching Unity yet
- No LOD, chunking or pixel error / basemap distance (every heightmap sample is a vertex); no holes, trees,
  details/grass, wind, neighbour terrains (Create Neighbor), stamp or custom brush shapes, or terrain layer assets.
- Max 4 layers; no per-layer mask maps, height-based blending, specular colour, or "Paint Holes".
- Rotation/scale of the object are ignored (as in the reference) but not hidden in the Transform Inspector.
- Brushes don't work in Play mode; Set Height has no Shift-sample of the height under the mouse; no brush
  rotation, spacing or scatter; strengths are our own rates.
- Wireframe draw modes don't draw the terrain's wires; terrains have no selection outline.
- No `TerrainData` API in Lua beyond the read-only table (no `GetHeights`/`SetHeights`, `GetInterpolatedNormal`).

## Materials (Standard shader options, Unlit shaders) ✅

### Done
- Standard shader Rendering Mode: Opaque, Cutout (Alpha Cutoff, clipped in the shadow map too), Fade (everything
  fades) and Transparent (premultiplied: highlights and reflections stay, alpha grows with reflectivity).
  Fade/Transparent don't write depth and are drawn back to front after opaque meshes, terrains and the grid and
  before particles; opaque (queue 2000) then cutout (2450) are drawn front to back. Pipelines per variant are
  built on first use.
- Maps: Albedo (alpha = transparency, or smoothness with Source = Albedo Alpha), Metallic map (R = metallic,
  A = smoothness, the Smoothness slider scales it), Normal Map, Occlusion (G, with strength; ambient and
  reflections only), Emission map × HDR emission colour, Tiling/Offset for all main maps; Double Sided (Cull Off,
  back faces lit from their side) as an extra.
- Shaders `Unlit/Color`, `Unlit/Texture`, `Unlit/Transparent` (fogged, no lighting, no shadows).
- Material Inspector laid out like the Standard shader (Shader dropdown, Rendering Mode, Main Maps, Advanced
  Options with the Render Queue) with a live preview at the bottom (engine renderer, sphere or cube, drag to
  rotate). The Project window shows rendered material thumbnails (cached until the file changes).
- Mesh Renderer Materials list with Size: Element 0 is the old `material` field; extra elements draw the mesh
  again (single-submesh behaviour). Older scenes and `.mat` files load and re-save unchanged.
- Lua: `MeshRenderer` / `Renderer` with `material` (per-object copy), `sharedMaterial`, `materials`,
  `sharedMaterials`; `Material` with `color`, `mainTextureOffset/Scale`, `shader`, `SetColor/SetFloat/…` with
  the Standard property names and the `_EMISSION` keyword. Play mode only.
- Sample: `Scenes/MaterialsDemo.scene` (glass, pulsing ghost via `MaterialPulse.lua`, cutout lattice sphere and
  fence, metallic-mapped sphere, unlit sign, emission-mapped panel) with generated textures.
- Tests: draw order, pipeline state per mode, shader names, old file re-save, option round trips, unlit and
  shadow rules, Lua instance/shared/list API, preview cache, sample generator check and headless play.

### Not matching Unity yet
- No Secondary Maps (detail albedo/normal), Height (parallax) map, Specular setup shader, Specular Highlights /
  Reflections toggles, GPU instancing or Double Sided GI; one Smoothness value is both Glossiness and the map
  scale.
- Fade/Transparent materials cast no shadows (the reference dithers them); transparent meshes are sorted by
  object origin and separately from particles (always before them), not by bounds centre in one list.
- No custom Render Queue override, no shader keywords beyond `_EMISSION`, no material property blocks, no
  material variants, no `new Material(…)` / `Shader.Find` in Lua; `sharedMaterial` edits are not saved to the
  asset (the reference editor keeps them).
- The material preview has no lighting/background toggles or other preview shapes (cylinder, torus, plane);
  thumbnails don't refresh when only a referenced texture changes.

## Navigation ✅

### Done
- Window ▸ AI ▸ Navigation (on-demand tab next to the Inspector): Agents (radius, height, step height, max
  slope, with a side-view diagram), Bake (Manual Voxel Size, Min Region Area, Clear / Bake, progress bar and
  status line) and Object (Navigation Static of the selection) pages, Show NavMesh toggle.
- Bake from active Static objects: Mesh Renderers (primitives and imported models) and Terrains; voxel
  heightfield, slope/clearance/step filters, erosion by the agent radius, small islands dropped, planar
  rectangle polygons with portal links. Runs on a worker thread; saved as `<Scene>/NavMesh.navmesh` next to the
  scene and referenced from the scene file (`"navigation"` block).
- Scene view: translucent blue nav mesh while the Navigation window is open, agent cylinder and obstacle
  gizmos for the selection, yellow path of selected agents in Play mode. `.navmesh` assets show a 🚶 icon.
- `Nav Mesh Agent` (Component ▸ Navigation): base offset, speed, angular speed, acceleration, stopping
  distance, auto braking, radius, height, avoidance quality and priority. A* + funnel paths, partial paths to
  unreachable targets, height following on ramps and steps, separation between agents, warps when scripts move
  the Transform.
- `Nav Mesh Obstacle`: box/capsule footprint agents steer around (no carving).
- Lua: agent `SetDestination`, `destination`, `remainingDistance`, `hasPath`, `pathPending`, `pathStatus`,
  `isStopped`, `velocity`, `speed`…, `ResetPath`, `Warp`; `NavMesh.SamplePosition`, `NavMesh.CalculatePath`;
  `Camera.main:ScreenPointToRay` / `WorldToScreenPoint`, `Input.mousePosition`.
- Sample: `Scenes/NavigationDemo.scene` (maze, ramp to a platform, stepped terrace, patrols, chasers, click to
  move their target) with `Patrol.lua`, `Chaser.lua`, `ClickMarker.lua`.
- Tests: bake area/erosion, slope limit, holes and dropped tops, step height, paths around walls (on the mesh,
  shortest), partial/invalid paths, file round trip and validation, scene settings round trip, agents reaching
  destinations around walls and up ramps, separation and stopping distance, warp/reset/teleport, obstacle
  avoidance, Lua agent/NavMesh/Camera API, Navigation window menu + bake/clear, sample generator check and a
  headless play of the demo (including a simulated click).

### Not matching Unity yet
- Polygons are grid-aligned rectangles (stair-stepped edges, more polygons than a contour-traced mesh); no
  detail/height mesh, no tiles, no runtime or partial rebakes, no NavMeshSurface/Modifier/Link components.
- One agent type (Humanoid) whose settings live in the scene; no areas, area costs or area masks (mask
  arguments are accepted and ignored), no off-mesh links or generated jump/drop links.
- Obstacles don't carve; avoidance is simple separation (no velocity obstacles), all qualities behave alike.
- Agents: no auto repath, `updatePosition`/`updateRotation` switches, `Move`, agent `CalculatePath`,
  `nextPosition` setter or `isPathStale`; paths are computed immediately.
- No `NavMesh.Raycast` / `FindClosestEdge` and no NavMeshPath object (CalculatePath returns a corner list).

## Code Editor (built-in Lua editor) ✅

### Done
- Window ▸ General ▸ Code Editor: an on-demand tab that docks with the Scene and Game views. Double-clicking a
  `.lua` file in the Project window, Inspector ▸ script ⋮ ▸ Edit Script (or double-clicking the Script field),
  and Project ▸ Create ▸ Lua Script open the script here. Edit ▸ Preferences ▸ External Tools ▸ Script Editor
  switches back to the system's default app; "Open in External Editor" stays in the tab and Inspector menus.
- Several files as tabs with a ● dirty marker, × / middle-click / Ctrl+F4 close, Ctrl+Tab cycling, a context
  menu (Save, Close All But This, Copy Full Path, Select in Project); Save / Don't Save / Cancel when closing a
  modified file and when quitting the editor. Files changed on disk reload (or ask when modified); renamed and
  moved scripts keep their tab.
- Editing: Lua highlighting (keywords, strings, `--[[ ]]` / `[==[ ]==]` long comments and strings, numbers,
  Misty globals such as `Input`/`Time`/`Physics`/`Vector3`, Lua builtins, function names), line numbers with the
  caret line highlighted, current-line band, bracket matching, indent guides, occurrences of the word under the
  caret, auto-indent after block openers and auto-dedent of `end`/`else`/`until`/`}`, auto-closing brackets and
  quotes (typing over closers, wrapping selections), Tab / Shift+Tab indent, monospace font with Ctrl+wheel /
  Ctrl+= / Ctrl+- zoom, word wrap, minimap (click/drag to scroll), folding by block from the gutter arrows.
- Visual Studio style shortcuts (full list under ? / F1 in the tab): Ctrl+S / Ctrl+Shift+S, undo/redo per file
  grouped by typing bursts, line-mode Ctrl+X/C/V, Ctrl+D and Ctrl+E,V duplicate, Ctrl+L / Ctrl+Shift+L,
  Alt+Up/Down, Ctrl+K,C / Ctrl+K,U / Ctrl+/, Ctrl+F / Ctrl+H (match case, whole word, regex with `$1`), F3 /
  Shift+F3 / Ctrl+F3, Ctrl+G, Ctrl+Shift+F (Find in Files, click a result to jump), smart Home, word moves,
  Ctrl+Enter / Ctrl+Shift+Enter, Ctrl+Space, Ctrl+K,D format document, F12 / Ctrl+click go to definition,
  Ctrl+] matching bracket, Ctrl+M,O / M,L / M,M / M,P outlining, Ctrl+U / Ctrl+Shift+U case, multi-caret
  (Alt+click, Ctrl+Alt+Up/Down, Shift+Alt+. / ;) and box selection (Alt+Shift+arrows or drag), F5 / Shift+F5
  Play / Stop.
- Autocomplete of keywords, names in the file and the Misty Lua API (globals, `self.`, `self.transform:`,
  `GetComponent("…")` names, variables assigned from `GetComponent`, raycast hits) with signatures and docs in the
  popup; hovering an API name shows its signature.
- Syntax errors are checked 0.4 s after typing stops (compiled, not run): red squiggle, gutter marker with the
  message on hover, and the status bar (click to jump). Console messages naming `Assets/….lua:N` get a link
  (and double-click) that opens the script at that line.
- Saving in Edit mode hot-reloads the script as before; in Play mode the game keeps its version until Stop.
- Tests: buffer, every command (duplicate/move/delete/comment lines, smart home, indent, auto-close/indent,
  paste modes, format), find/replace incl. regex, undo grouping, completion context and ranking, syntax error
  placement, Console links, chords; kittests for opening from the Project window and the Console, typing, the
  dirty marker, Ctrl+S on a temp project copy, the find bar, Go To Line, prompts, disk reloads, diagnostics,
  completion and F5.

### Not matching Visual Studio yet
- No debugger (breakpoints, stepping, watches): F5 only enters Play mode.
- No IntelliSense across files, no type inference beyond simple assignments, no parameter info while typing
  arguments, no rename refactoring, Find All References or Navigate Backward/Forward.
- Only the first syntax error is shown; runtime errors aren't squiggled.
- Undo history is lost when a file is closed; no split views, no drag-and-drop of text, no Ctrl+Shift+Up/Down
  selection expansion; word-wrap Up/Down moves by lines; the minimap is scaled to fit rather than scrolled.

## Performance tooling + first optimisations ✅

### Done
- Engine `profiler`: `profile_scope!("Name", Category::…)` (one atomic load while not recording),
  per-frame samples with nesting, a 300-frame history, self time per category, a merged
  Hierarchy tree and a JSON Lines capture writer. Scopes cover the player loop: Scripts
  Start/FixedUpdate/Update/LateUpdate and every script method ("Rotator.Update()"), physics
  step and transform sync, navigation, animation, audio, particles, UI events/layout/render,
  scene gathering and the renderer's prepare / culling / encode stages.
- GPU frame time from timestamp queries (first and last pass of each render, read back
  asynchronously) when the adapter supports them; missing otherwise.
- Window ▸ Analysis ▸ Profiler (Ctrl+7, docks with the Project/Console): Record (on when first opened), Clear, previous/next/
  Current frame; CPU Usage stacked chart by category (Rendering, Scripts, Physics, Animation,
  Navigation, Audio, UI, Others) with legend toggles, 60/30/15 FPS lines and the GPU line;
  Rendering (batches) and Memory (GPU MB) charts; click a frame to select it; Hierarchy
  (Total %, Self %, Calls, Time ms, Self ms) or Timeline view; Rendering counters (batches,
  saved by batching, SetPass, tris, verts, texture binds, shadow casters, visible/culled) and
  Memory (objects, meshes, textures, GPU buffer/texture estimates). Works in Play mode.
- Player: `MISTY_PROFILE=capture.jsonl` records every frame to a capture file. Editor capture env:
  `MISTY_STATS=1` shows the Stats box, `MISTY_TAB` accepts a list (`Game,Profiler`).
- Game view Stats box: FPS, CPU main / render, GPU, Batches and Saved by batching, Tris,
  Verts, Screen, SetPass calls, shadow casters, visible/culled meshes, audio voices.
- Renderer: frustum culling of meshes, terrains (whole) and particle systems per camera and
  per shadow map (spot, each point-light face, directional); automatic GPU instancing of
  identical mesh + shader variant + material textures in the opaque/cutout queues and shadow
  passes (transparent only merges neighbours, keeping back-to-front order) through a draw-slot
  indirection buffer; reused per-frame buffers; terrain bounds cached per version.
- Headless GPU tests: culling + instancing on vs off (and each alone) produce identical
  pixels; batches and culled counts checked.
- Sample: `Scenes/StressDemo.scene` (5,040 cubes and spheres, 4 materials, 200 spinners,
  orbiting camera via `CameraOrbit.lua`, sun + spot + two point lights).

### StressDemo, 1280×720, release build (`stress_benchmark`, headless, this machine)
| Configuration | FPS | Frame ms | Gather ms | Render CPU ms | Draw calls | Tris |
|---|---|---|---|---|---|---|
| Before (no culling, no instancing) | 49.4 | 20.3 | 8.6 | 9.0 | 15,123 | 5.9 M |
| Culling only | 53.1 | 18.8 | 7.5 | 5.2 | 9,685 | 3.8 M |
| Instancing only | 66.8 | 15.0 | 6.2 | 2.1 | 9 | 5.9 M |
| Culling + instancing | 93.5 | 10.7 | 6.2 | 1.9 | 9 | 3.8 M |

Frame = gather (scene → draw list) + render call + waiting for the GPU. The remaining CPU cost
is mostly gathering the scene into draws each frame.

### Not matching Unity yet
- No Deep Profile, thread rows, GC/allocation counters, editor-side capture save/load, or
  Profiler modules for Physics/Audio/UI details; Rendering/Memory describe the Game view only;
  GPU time excludes the editor UI.
- No occlusion culling, LOD groups, per-layer cull distances, static batching, SRP-batcher-
  style material batching, per-material "Enable GPU Instancing" toggle or material property
  blocks; the directional shadow map is fitted to all casters (no cascades), so it culls nothing.
- The scene → draw list gathering still runs every frame for every object (no caching of
  unchanged transforms/materials).

## Input Manager, gamepads, Tags and Layers ✅

### Done
- Edit ▸ Project Settings ▸ Input Manager: Axes list (Size, one foldout per axis with name, descriptive names,
  negative/positive/alt buttons, gravity, dead, sensitivity, snap, invert, type, axis, joy num; Duplicate /
  Delete), saved in `ProjectSettings.json`; the usual default axes.
- Runtime axes with gravity/sensitivity smoothing and snap; mouse movement and wheel axes; joystick axes with
  dead zones. Lua `Input.GetAxis`, `GetAxisRaw`, `GetButton/Down/Up`, `GetMouseButton/Down/Up`,
  `mouseScrollDelta`, `anyKey`, `anyKeyDown`, `GetJoystickNames()`; `"mouse 0"` / `"joystick button 0"` /
  `"joystick 1 button 0"` key names.
- Gamepads via `gilrs` in the player and the editor while playing (Xbox-style button/axis layout).
- Edit ▸ Project Settings ▸ Tags and Layers: user tags (+ / −), 32 layers (0–7 built in, 8–31 nameable). The
  Layer Collision Matrix, Inspector Tag/Layer dropdowns ("Add Tag…" / "Add Layer…" open the page) and
  `LayerMask.NameToLayer/LayerToName/GetMask` use them. Lua `GameObject.FindWithTag`,
  `FindGameObjectsWithTag` (and `CompareTag`).
- Camera ▸ Culling Mask (Nothing / Everything / per-layer toggles; Lua `cullingMask`) honoured by the game
  view and the player for meshes, terrains and particles.
- Sample: `Scenes/InputDemo.scene` (`InputMover.lua`, `GamepadHint.lua`, a Hidden-layer marker the camera
  skips; layer 8 "Hidden" in the sample's Project Settings).
- Tests: axis smoothing / snap / invert / gravity, mouse and wheel axes, buttons, key-name parsing, gamepad
  mapping and press detection with fake pads, settings round trip and old files, tags/layers sanitising and
  old layer indices, culling mask (and old cameras = Everything), Lua Input/tag/layer API, Project Settings
  pages, sample generator check and headless play with injected keys and a fake gamepad.

### Not matching Unity yet
- No light culling masks, no Sorting Layers, no Scene view Layers visibility/lock menu.
- Fixed Xbox-style layout for every pad (no per-device raw axes, no remapping, no rumble); only 10 joystick
  axes and 20 buttons, up to 8 joysticks.
- Mouse buttons above 2, `Input.GetKey(KeyCode…)` enums, `Input.inputString`, touch, accelerometer and the new
  action-based input system are not available; the Input Manager has no right-click array menu (Duplicate /
  Delete buttons instead).
- Renaming a tag or layer doesn't update objects using it; tags aren't reorderable.

## 2D: sprites, Sorting Layers, 2D physics (paused, WIP)

Status: paused at the user's request. Everything below is implemented; `cargo build`, `cargo test --workspace`
(engine 298, editor 144 + 13 ignored, player 4) and `cargo fmt` passed, and the editor, texture Inspector and
player were checked in screenshots. Left: re-run `cargo clippy --workspace --all-targets` after the last two
clippy fixes in `sprite_handles.rs` (a type alias, a removed `.clone()`), then the merge.


### Done
- Texture import settings per image in a `<image>.meta` sidecar: Texture Type Default / Sprite (2D and UI),
  Sprite Mode Single / Multiple, Pixels Per Unit, Pivot, Filter Mode Point / Bilinear, Wrap Mode, Generate Mip
  Maps, sliced sprite rects. Textures without a `.meta` behave as before. Selecting an image shows the import
  Inspector (Revert / Apply, Slice: Grid By Cell Size, preview with the sprite rects outlined).
- `Sprite Renderer` (sprite, colour, flip X/Y, Draw Mode Simple, Sprites-Default material, Sorting Layer, Order
  in Layer): unlit alpha-blended two-sided quads in the transparent queue, sorted by Sorting Layer, Order in
  Layer, then distance. Built-in Square / Circle / Triangle sprites. Project Settings ▸ Sorting Layers.
- GameObject ▸ 2D Object ▸ Sprite and Sprites ▸ Square / Circle / Triangle; sprite picking (top-most wins),
  framing, the Rect tool on sprites (resize by scale, drag to move); 2D Scene view (2D toggle) and
  orthographic cameras.
- UI Image `sprite` (draws the sprite's rect).
- 2D physics on rapier2d, separate from 3D: `Rigidbody 2D` (Dynamic / Kinematic / Static, mass, linear and
  angular drag, gravity scale, collision detection, freeze position / rotation), `Box`, `Circle`, `Capsule`,
  `Polygon` (points or the sprite rect, concave decomposed) and `Edge Collider 2D` with Is Trigger, Offset and
  Physics Material 2D (friction, bounciness); layers via the Layer Collision Matrix. Project Settings ▸
  Physics 2D (gravity, query options). Green collider outlines and Edit Collider handles for Box / Circle.
- Lua: `OnCollisionEnter2D/Stay2D/Exit2D(collision)`, `OnTriggerEnter2D/Stay2D/Exit2D(other)`, `Rigidbody2D`
  (`velocity`, `angularVelocity`, `AddForce`, `AddTorque`, `MovePosition`, `MoveRotation`, `bodyType`,
  `gravityScale`, …), 2D colliders, `SpriteRenderer`, `Physics2D.Raycast/RaycastAll/OverlapCircle(All)/
  OverlapBox/OverlapPoint`, `Physics2D.gravity`, `Vector2`, `ForceMode2D`.
- Sample: `Scenes/Platformer2D.scene` (generated pixel-art sprites, tile platforms, coins, score text, camera
  follow; A/D + Space).
- Tests: import settings round trips and repairs, PPU sizing and pivots, grid slicing, Multiple-mode lookup,
  uv rects and flips, sprite draws and sorting, built-in shapes, settings round trip; 2D falling and landing,
  body types, MovePosition, forces, freeze rotation, collision enter/stay/exit with contact data, triggers,
  raycasts / overlaps / masks / query settings, every collider shape, bounciness, parent bodies and layers;
  Lua 2D API; editor Rect tool and collider handles; sample generator check and a headless play.

### Not matching Unity yet
- No Sprite Editor window (slicing is in the texture Inspector, grid only), no sprite atlases, Sliced/Tiled
  draw modes, sprite masks, custom sprite materials, sprite shapes, tilemaps or 2D lights.
- No GUIDs: references are paths, so renaming an image outside the editor breaks its sprites.
- No Physics Material 2D assets, joints 2D, effectors, composite colliders, auto-fit of new colliders to the
  sprite, Edit Collider for capsule/polygon/edge, `Rigidbody2D.Sleep`/`IsTouching`, `Physics2D.BoxCast`/
  `CircleCast`, contact filters, or "Use Full Kinematic Contacts"; Collision2D has a single contact point.
- Sorting Layers have their own Project Settings page (not Tags and Layers); sprites take fog.

## Soft shadows: PCSS and cascaded sun shadows ✅

### Done
- Contact-hardening soft shadows (PCSS) for directional, spot and point lights: blocker search, penumbra from
  the light's size, variable-size PCF on a per-pixel rotated Vogel disk, receiver-plane depth bias. Shadows are
  sharp where a caster touches the ground and widen with distance. Hard / Soft / None kept (Soft = PCSS).
- Light Inspector ▸ Shadow Angle (directional, degrees) or Shadow Radius (spot/point, metres), Bias, Normal Bias.
- Cascaded sun shadows: 1–4 cascades (default 4), practical split, blending between cascades, stabilised with a
  bounding sphere and texel snapping, fade-out at the Shadow Distance; frustum culling and instancing per
  cascade. This replaces the single map fitted around every caster, which blurred shadows in large scenes.
- Project Settings ▸ Quality: Soft Shadow Quality (Low/Medium/High/Ultra), Shadow Distance, Shadow Cascades,
  Cascade Split, Cascade Blend (the player uses them too).
- Scene view draw mode "Shadow Cascades" (one colour per cascade).
- Sample: `Scenes/ShadowsDemo.scene` (poles, a figure, a spot-lit table, pillars into the distance, low sun).
- Tests: split distances, every slice corner inside its cascade, whole-texel snapping while the camera moves and
  turns, caster depth range, quality settings clamping and old files; GPU: the penumbra widens with distance
  (hard shadows don't), ShadowsDemo shows all four cascade tints; the culling/instancing pixel-identity test
  still passes with cascades.
- Screenshots: `docs/screenshots/round8/shadows_*.png` (before/after Game view, cascade debug in the Scene view
  and from the game camera, a close-up of contact hardening, the top-down pole penumbra test image, the Quality
  page with the spot light's Inspector).

### Not matching HDRP yet
- No temporal filtering: the per-pixel disk rotation is static noise (fine grain on wide penumbrae at Low and
  Medium), where HDRP smooths it with TAA.
- Point lights use six perspective faces of the shared 1024² array (wide penumbrae can show a seam at face
  edges); no per-light shadow resolution, shadow atlas or caching, coloured/translucent shadows or area-light
  shadows. Contact shadows are another branch.
- Cascade splits use one λ instead of per-cascade percentages; no per-cascade border controls, per-light
  Shadow Strength / near plane, shadowmask or baked shadows.
- The blocker search uses point samples, so very thin casters far above the receiver can slip between taps.

## Reflection probes with automatic placement ✅

`cargo test --workspace`: engine 338 (+1 ignored), editor 206 (+16 ignored), player 4; clippy and fmt clean.
Screenshots: `docs/screenshots/round8/probes_*.png` (before/after Game view, Scene view, debug view, Lighting).

### Done
- `Reflection Probe` component (GameObject ▸ Light ▸ Reflection Probe): Baked / Realtime (refresh every N
  frames or on demand, time sliced one face per frame), importance, intensity, box projection, blend distance,
  box size / offset, resolution 64–512, near / far clip, clear flags Skybox / Solid Color. Inspector with Bake,
  Refresh Now and Edit Bounding Volume (face handles in the Scene view).
- Baking with the engine's renderer (six HDR faces), optional second bounce, GGX-prefiltered roughness chain
  (filtered importance sampling) and SH9 diffuse; saved as compressed half-float `ReflectionProbe-N.probe`
  next to the scene and loaded in the editor and the player.
- Shading: per-draw lists of up to four probes, per-pixel blend by importance / box size / blend distance, box
  projection, roughness mip for specular with a split-sum BRDF LUT (plus multiple-scattering energy), probe
  SH for diffuse ambient; sky fallback unchanged where no probe covers a pixel (scenes without probes render
  as before).
- Automatic placement (Lighting ▸ Scene ▸ Generate Probes, GameObject ▸ Light ▸ Generate Reflection Probes):
  floors found by raycasts over the renderer bounds, probes 1.5 m above them, solids skipped, boxes fitted to
  walls / floor / ceiling with median ray fans, rooms and open areas merged; placed under an undoable
  "Reflection Probes" group (settings: spacing, height, merge, resolution, second bounce), then baked with a
  progress bar. Realtime probes update in the editor and the player.
- Gizmos: chrome sphere per probe, box + blend box for the selection, Scene view draw mode "Reflection Probes".
- Sample: `Scenes/ReflectionsDemo.scene` (coloured room with a doorway, chrome / gold / copper spheres, glossy
  tiles, outdoor lawn and chrome ball; 10 generated probes, baked data committed).
- Tests: cube face math, SH projection vs brute force, prefilter energy / blur, BRDF LUT values, probe file
  round trip, BVH, placement (probes out of walls and the pillar, boxes fit the test room, merging,
  determinism), per-draw list order, GPU capture orientation, a chrome sphere in a red room reflecting red,
  Lighting window Generate + undo, sample generator check and a headless render of the demo with its probes.

### Not matching HDRP yet
- One box per probe (influence = projection box, axis-aligned); no sphere shape, separate proxy volume,
  per-face blend distances, normal-direction blend or oriented boxes.
- Prefiltering and SH run on the CPU (fine for bakes, a few frames of latency for realtime probes); no GPU
  convolution, no probe atlas compression (BC6H), no planar reflections or screen-space reflections.
- No light probes / APV: diffuse comes from the reflection probes' SH only (no per-object light probe
  interpolation); probes capture every active renderer, not only Static ones.
- Specular occlusion is just the material's occlusion; no horizon fade or bent normals.

## Volumetric fog and light shafts ✅

### Done
- Froxel volumetric fog (`render/volumetrics/`): camera-aligned grid with exponential depth slices (Low
  96×54×48, Medium 128×72×64, High 160×90×64) built in two compute passes: density (global height fog with
  base height and falloff, up to 16 Fog Volumes with soft edges, scrolling 3D noise) lit by up to 8
  directional / spot / point lights through their shadow maps with a Henyey-Greenstein phase plus sky ambient,
  temporal reprojection with Halton jitter; then front-to-back integration into an in-scatter / transmittance
  volume. Meshes (all modes, unlit too), terrain, the sky and background, and alpha / additive particles sample
  it in their shaders (`fog_apply.wgsl`).
- Analytic height fog (same density, unshadowed sun + ambient) when Volumetric is off, in orthographic views,
  and between the volumetric distance and the max fog distance.
- Post-process Volume ▸ **Fog**: Enable, Max Fog Distance, Fog Attenuation Distance, Base Height, Maximum
  Height, Albedo, Anisotropy, Ambient Light Probe Dimmer, Volumetric, Volumetric Distance, Quality, Denoising
  (temporal), Noise Intensity / Scale / Scroll Speed. **Sun Shafts**: Intensity, Threshold, Length, Radius, Tint
  (screen-space radial blur of the bright open sky around the sun, half resolution).
- Light Inspector: Volumetric Multiplier, Volumetric Shadow Dimmer.
- `Fog Volume` component (Size, Fog Distance, Albedo, Blend Distance), GameObject ▸ Rendering ▸ Local
  Volumetric Fog, Component ▸ Rendering; selected boxes show their outer and inner (blend) box; ☁ scene icon.
- Scene view draw modes **Fog Density** and **Fog Lighting**; the Fog effects toggle controls volumetric fog.
- Sample: `Scenes/VolumetricFogDemo.scene` (wall with window slits + forest against a low sun, street-lamp spot
  cone, lantern point light, a fog-filled valley, orbiting camera).
- Tests: shaders validate, uniform layouts, slice mapping, phase normalisation, analytic height-fog integral vs
  numeric, old volumes / lights load with fog off and save unchanged, Fog Volume weights, uniform packing
  (multiplier 0 skipped, dimmer, box cap, jitter); headless GPU: shadowed fog darker than lit fog, shadow
  dimmer 0 / multiplier 0, spot cone visible, Fog Volume in the density view, froxel = analytic = temporal on
  unshadowed fog, fog off pixel-identical to a renderer that never had fog, sun shafts brighten / skip when the
  sun is behind; sample check + headless play.

### Cost (`fog_benchmark`, release, RTX 4060, 1920×1080, VolumetricFogDemo, whole frame GPU time)
| Fog | GPU ms / frame | Fog cost |
|---|---|---|
| Off | 0.92 | — |
| Low (96×54×48) | 1.08 | 0.16 |
| Medium (128×72×64) | 1.09 | 0.16 |
| High (160×90×64) | 1.11 | 0.19 |

Screenshots: `docs/screenshots/round8/fog_before_off.png`, `fog_after_godrays.png`, `fog_scene_view.png`.

### Not matching HDRP yet
- No depth-based froxel culling, no "slice distribution uniformity" control (fixed exponential), no
  per-froxel multiple scattering / anisotropy per volume, no 3D density textures on Fog Volumes, no volumetric
  clouds, no light probe ambient (sky ambient only), no area lights or light cookies in the fog.
- Volumes aren't blended between overlapping local post volumes (only global volumes exist; the highest
  priority wins as a whole).
- Orthographic views get analytic fog only; reflections don't see fog.

## Ambient occlusion and contact shadows ✅

### Done
- Screen-space ambient occlusion, horizon based (GTAO-style): depth + normal prepass (only while an effect
  is on), half- or full-resolution trace, depth/normal-aware denoise, bilateral upsample, temporal
  accumulation with reprojection and camera-cut reset (wider spatial blur as the fallback).
- Applied to ambient diffuse (with the material occlusion map, multi-bounce) and as specular occlusion on
  reflections; direct light only through Direct Lighting Strength (default 0).
- Post-process Volume overrides: **Ambient Occlusion** (Intensity, Radius, Direct Lighting Strength,
  Quality (Steps), Full Resolution, Temporal Accumulation) and **Contact Shadows** (Length, Distance Fade,
  Opacity, Sample Count); Light Inspector **Contact Shadows** toggle. All off in existing scenes.
- Scene view draw mode **Ambient Occlusion** (debug view).
- Sample: `Scenes/AmbientOcclusionDemo.scene` (corner room, crates, columns, spheres, cubes at rising
  heights, a bobbing sphere).
- Tests: settings defaults/clamps, arc integral, multi-bounce, AO shader validation, camera cuts; GPU: floor
  darker beside a resting sphere than a floating one, crease darkening fading with distance (full and half
  resolution), temporal converging, contact shadows beside a cube, AO debug view, and effects off rendering
  bit-identically to a renderer that never ran them; demo generator check, play test and GPU render test.
- Screenshots: `docs/screenshots/round8/ao_*.png`.

### Not matching HDRP yet
- Normal maps don't reach the prepass (AO uses vertex normals); no "thickness" heuristic for thin occluders,
  no bent normals, no ray-traced AO.
- Volumes don't blend field by field (the highest-priority volume wins, like the other overrides).
- Contact shadows have no separate Thickness / Min Distance / Fade In settings and don't run for
  transparent surfaces; screen-space limits apply (nothing off screen casts them).

## Player, Quality and the build pipeline ✅

### Done
- Project Settings ▸ **Player**: product name, company, version, icon (PNG from Assets), optional splash screen
  (image, duration, background), screen mode (Fullscreen Window / Exclusive Fullscreen / Windowed / Maximized),
  default resolution, Resizable Window, Run In Background, VSync, Target Frame Rate. The player applies all of
  it at startup (window icon too). Older projects migrate the old Build Settings product/size/fullscreen.
- Project Settings ▸ **Quality**: editable list of levels (Low / Medium / High / Ultra, add / remove / rename,
  one default) with shadows (resolution, distance, cascades, soft), rendering (MSAA, render scale, LOD bias),
  textures (texture quality = global mipmap limit, anisotropic textures, max texture size) and effects (AO,
  volumetric fog, reflection probes). `misty_engine::quality` holds the active level; `Renderer::set_quality`
  resizes the directional shadow map; render scale, texture limits and anisotropy are applied; the rest is
  exposed through accessors for the shadow / AO / probe / fog work. Game view toolbar dropdown previews a level.
- Project Settings reorganised into six pages: Player, Quality, Input, Physics (3D + collapsed 2D section),
  Tags and Layers (+ collapsed Sorting Layers), Time; each with a description and collapsible sections.
- Texture import: Filter Mode Nearest Neighbor / Bilinear / Trilinear (old "Point" loads), Max Size 32–8K with
  Lanczos downscale, Normal Map type, sRGB toggle, Aniso Level 0–16 under a collapsed Advanced section; Apply
  reloads live. Trilinear + anisotropic sampling in the Scene view, Game view and player.
- Lua: `Screen` (width, height, fullScreen, fullScreenMode, resolutions, SetResolution), `FullScreenMode`,
  `QualitySettings` (GetQualityLevel, SetQualityLevel, names), `PlayerPrefs` (Get/Set Int/Float/String, HasKey,
  DeleteKey, DeleteAll, Save). The player remembers the chosen quality and resolution in the per-user prefs.
- File ▸ Build Settings: platform list (Windows x64/ARM64, Linux x64/ARM64, macOS Apple Silicon/Intel) with
  `rustup` checks and the exact `rustup target add` command, cross-OS blockers explained; Development Build,
  Compress Data, Create .zip, Steam App ID; Build / Build And Run on a background thread with a progress bar
  and log (cargo output streamed).
- Build output: `<Product>.exe` with the icon embedded (Windows), `<Product>_Data/data.pak` (LZ4 pak with only
  the assets the build scenes use plus `Resources` folders, and the settings), icon files, macOS `.app`
  bundles, `steam_appid.txt` for development builds, `steam/` SteamPipe scripts + README, optional zip.
- All asset loading goes through `misty_engine::vfs` (pak or disk); loose project folders still play.
- docs/building.md: a beginner's guide to building, platforms, itch.io and Steam.
- Tests: pak round trip, vfs keys, dependency walk, quality accessors / sanitising, player settings migration,
  prefs file round trip, Lua Screen/Quality/PlayerPrefs, texture max size / filters / old `.meta` loading,
  target status and cargo command construction, icon files and embedding, zip, Steam templates, packaging
  layout (Windows and macOS), build job errors, the Build / Player / Quality windows, playing from a pak.

### Not matching Unity yet
- No Steamworks API (achievements, overlay), no IL2CPP-like options, no code signing / notarization, no
  `.icns` icon for macOS, no Linux `.desktop` file.
- MSAA level is stored but the renderer always uses 4x; LOD bias is stored (no LOD Groups yet).
- Render Scale scales the UI too. Screen.SetResolution does nothing in the Game view.
- Cross-compiling Linux needs cargo-zigbuild; macOS builds need a Mac.
- No per-platform overrides of texture import settings or Player settings.

## Terrain: Auto Paint, cliffs, grass, Color Harmony, large-terrain editing ✅

### Done
- Auto Paint Rules per layer (collapsible in Paint Texture): rule-driven flag, slope band (two-handle slider),
  optional height band, Spread, Breakup strength + scale (seeded fBm), Priority. **Generate** repaints the
  rule-driven layers over the whole terrain (multithreaded, one undo step) and keeps manual layers' strokes;
  **Generate in brush** makes the Paint brush apply the rule result locally. Hand painting still works on top.
- Cliffs & Blending: triplanar projection above an angle (default 45°) with blend sharpness, whiteout normal
  blending, a cheap top-only path when off or on gentle slopes; height-based blending with a Height Blend
  contrast slider (layer Height Map, or the albedo luminance).
- Grass & Details: same rules (slope, height, spread, breakup) plus "Grow On" layer and density; Generate fills
  a density map; blades are scattered per 16 m chunk around the camera and drawn instanced with wind sway,
  distance fade, density and draw-distance settings. Never on slopes past the limit + spread.
- Color Harmony: per-layer mean linear albedo and coverage → shared target (Average, Tint, Lighting); per-layer
  hue shift / saturation / brightness in the shader by Strength and H/S/B weights; Preview Before/After.
- Renderer: heightmap and normals as textures (vertex pulling), chunked LOD with crack-free edge stitching,
  per-chunk frustum culling, partial uploads of only the brush region, layer texture arrays. Heightmaps up to
  4097², splat maps up to 2048².
- Sample: `Scenes/TerrainDemo2.scene` (fBm mountains, terraced cliffs, meadow grass, snow band, rock on
  slopes, rule-driven layers, Color Harmony on; textures in `Assets/Textures/Terrain/`).
- Tests: rock on steep / grass on flat, breakup irregular and seeded, spread widens the transition, manual
  strokes kept, snow height band, generate-in-brush locality; detail placement (flat only, deterministic,
  chunk-additive, layer gating); harmony maths (HSV, linear mean, circular weighted mean, strength 0 identity,
  tint/lighting); LOD patterns cover chunks, stitched edges match coarse neighbours, LOD steps ≤ 1; noise;
  GPU: partial uploads give the same image as a fresh upload; TerrainDemo2 check, rules and play tests.

### Measured (release, RTX 4060, `terrain_timings`, profile scopes Terrain.*)
| Heightmap | Generate splat (1024²) | Generate details | First upload + 2 frames | Brush step (CPU) | 2 frames after a stroke (partial upload) |
|---|---|---|---|---|---|
| 2049² | 83 ms | 110 ms | 37 ms | 0.03 ms | 4.8 ms |
| 4097² | 58 ms | 32 ms | 163 ms | 0.02 ms | 4.7 ms |

Before this change every stroke frame rebuilt and re-uploaded the whole mesh (1025² max, ~50 MB of vertices
at 1025²); now a stroke uploads only the brush rectangle, so the cost no longer grows with terrain size.
Screenshots: `docs/screenshots/round8/terrain_final.png`, `terrain_no_breakup.png`, `terrain_no_harmony.png`,
`terrain_planar.png` (triplanar and height blend off), `terrain_demo2_editor.png`, `terrain_inspector_paint.png`.

### Not matching Unity yet
- No tree instances or detail meshes (grass blades only); no detail painting brush (Generate / Generate in
  brush only); grass casts no shadows.
- Layer textures skip import settings; at most 4 layers and 1024² slices.
- Saving a 4097² terrain on mouse-up takes a moment (JSON + base64).

## Round 8: material overrides + rendering performance ✅

### Done
- **Per-object material overrides** (Mesh Renderer, per Materials element): UV Tiling, UV Offset,
  UV Rotation, Color tint (multiplied), Smoothness / Metallic / Emission multipliers. Only overridden
  values are stored (`overrides` left out of files when empty). They go into the draw's own `draws[]`
  entry, so three walls sharing a material with different UVs/tints are still one instanced draw.
  Inspector: collapsible "Overrides (this object only)" under each element (opens when it has
  overrides), checkbox per property, bold ● rows, count in the header, Reset All; undo/redo and
  multi-object editing (nested values merge per field; mixed values show —). Lua:
  `renderer:SetOverride/ClearOverride/GetOverride(name[, element])`. Survive prefab Apply/Revert.
- Shared per-draw struct in `shaders/draw.wgsl` (layout documented, 224 bytes, one `extra` vec4).
- **Cached, incremental gather** (`DrawCache`, one per view): compares last frame's inputs, rebuilds
  only changed objects, recomputes world matrices only below a changed transform, resolves each distinct
  material once per frame; the draw list is shared (`Arc`), no per-frame copy. Lights, cameras, post
  volumes and terrains no longer walk the scene. Render.GatherScene on a static StressDemo: 0.8 ms
  (was 2.6–6 ms).
- **Parallel prep** (rayon): per-draw bounds, LOD state, keys and `draws[]` entries; every shadow map
  culled and batched on its own thread.
- **Static batching**: Static objects (no LOD Group) merged per material + 16 m chunk into world-space
  meshes in Play mode and the player (chunks still culled); `Scenes/StressStatic.scene`.
- **GPU culling** (option `gpu_culling`): two-phase frustum + Hi-Z occlusion in compute, indirect draws,
  `multi_draw_indexed_indirect` for runs sharing state (primitives share one vertex buffer); CPU path
  as fallback. Off by default (slower on StressDemo, which has little occlusion).
- **LOD Group** component: levels with renderer lists and screen-height thresholds, per-camera selection
  with `RenderOptions::lod_bias`, dithered Cross Fade; Inspector LOD bar (drag thresholds, camera
  marker, per-level renderers, Fill from Children). LOD demo row + override walls in StressStatic.
- **Depth prepass** (option `depth_prepass`): identical output; off by default (slower here).
- Editor: the Hierarchy lays out only visible rows (the expanded 5,040-object Field no longer costs
  milliseconds per frame); Scene and Game views use their own draw caches.
- GPU checks (real adapter): overrides → one batch and pixel-identical to separate draws; static
  batching (0 pixels differ), GPU culling (0 differ, wall hides most of the field) and prepass (0 differ)
  vs the plain path; LOD levels by distance, cross-fade, LOD Bias.

### StressDemo, 1280×720, release (`stress_benchmark`), same run (other agents' builds share the machine)
| Configuration | FPS | Frame ms | Gather ms | Render CPU ms | GPU ms |
|---|---|---|---|---|---|
| Baseline (no culling, no instancing, full gather) | 79.1 | 12.64 | 3.75 | 7.21 | 1.51 |
| Culling + instancing, full gather (previous best) | 154.4 | 6.48 | 2.58 | 2.28 | 1.45 |
| **+ cached gather + parallel prep (new default)** | **202.2** | 4.95 | 0.82 | 2.19 | 1.79 |
| default without instancing | 141.2 | 7.08 | 0.87 | 4.24 | 1.82 |
| default without culling | 165.7 | 6.03 | 1.00 | 2.67 | 2.20 |
| default + depth prepass | 151.2 | 6.62 | 1.05 | 2.96 | 2.41 |
| default + GPU culling (frustum + Hi-Z) | 171.3 | 5.84 | 0.77 | 2.42 | 2.47 |
| StressStatic, static batching (Play mode) | 329.0 | 3.04 | 0.86 | 0.59 | 1.48 |

An earlier run while the machine was busy gave the same ordering (48 → 42 → 64 fps; StressStatic 154).

### Not matching Unity yet
- Overrides are per renderer, not MaterialPropertyBlocks: no arbitrary shader properties, no texture
  swaps; terrains and sprites have none.
- LOD levels: bounds recomputed each frame (no stored reference point / size), no SpeedTree fade mode,
  unlit/transparent levels don't dither; shadows use the camera's level.
- GPU culling covers opaque main-pass draws only (shadow maps and transparent stay on the CPU);
  multi-draw only merges batches that share a vertex buffer (primitives).
- Static batching happens in Play mode/player only and isn't saved into builds; moving a Static object
  re-merges its batch instead of being ignored.

## World Builder ✅
- **Window:** Window ▸ Terrain ▸ World Builder is one window with collapsible sections (guide in
  `docs/world_builder.md`). It creates a World Builder with a new terrain, or uses the selected terrain.
- **Base Heightmap:** six generators; sliders for Height, Scale, Noise Amount, Roughness (octaves), Ridge
  Sharpness and Seed, plus Randomize. Noise is domain-warped fBm with ridged and billow variants.
  Hydraulic (droplet) and thermal erosion run on all cores, are deterministic and can be cancelled.
- **Stamps & Areas:** each is a scene object under "World Builder":
  - Mountain/Hill (cone, ridge, volcano, mesa; add / max / min / replace)
  - Flatten (object or sampled height)
  - River/Valley spline (width, depth, bank, water strip)
  - Water Body (lake flood fill or sea level)
  - Rock Area
  - Biome Zone (circle or polygon)

  Their outlines are draped on the terrain in the Scene view, with dot handles for radius, falloff and
  points.
- **Biomes:** `.biome` assets in `Assets/WorldBuilder/` hold:
  - flat / slope / cliff / shore / peak textures with tiling
  - classification thresholds
  - scatter lists (prefab or primitive shape; density, Poisson spacing, scale and rotation randomness,
    slope and height limits, align to ground)
  - a colour tone

  Six presets with generated textures; biome edges blend with noisy breakup.
- **Generate:** builds height → erosion → stamps → water → splat → scatter, as one undo step. Heights /
  Textures / Scatter Only redo one part. A low-res live preview runs (debounced) while editing, with Revert.
- **Scatter:** stored as objects with shared materials, or compact `Scatter` components (14 bytes per
  instance). Both are drawn through automatic instancing. Water is a simple transparent plane behind
  `world_gen::water::spawn_water`.
- **Sample:** `Scenes/WorldBuilderDemo.scene`, a mountain island with a crater lake, a river, a village
  plateau, forest / snowy-peak / beach biomes and the sea.
- **Tests:**
  - Engine: determinism, flatten flatness, river carving, erosion (mass, channels, cancel), lake fill stays
    in its basin, scatter slope limits and spacing, classification, biome zones, blend modes, stages, scene
    round trip, presets.
  - Editor: menu, create, generate + undo, preview + revert, stamp inspector.
  - Sample: generator check and a headless play.
- **Timings** (release, 16 threads): 1025² erosion takes about 1.5 s hydraulic + 0.4 s thermal; generating
  the demo takes a few seconds.

### Not done yet
- No GPU erosion, heightmap-image stamps or spline roads.
- The terrain still has at most four splat layers.
- Water is a plain plane (the ocean system takes over `spawn_water`).
- Edits to biome assets are not in undo.

## Custom shaders and the water system ✅

### Done
- `.shader` assets (WGSL) with a comment header: `@shader surface|unlit`, `@blend`, `@cull`, `@zwrite`, `@queue`,
  `@property NAME float|range(a,b)|color|vector|texture2d ["Label"] = default // tooltip`. Guide: `docs/shaders.md`.
- Surface shaders (`fn surface(SurfaceInput) -> SurfaceOutput`) go through the engine's lit `shade()`, so they get
  lights, shadows, ambient, sky reflections and fog. Unlit shaders use `fn fragment` and return the final colour.
  An optional `fn vertex(VertexData)` displaces vertices. Built-ins: `globals.time`/`screen`, the view uniform, object
  matrices, and the opaque scene's colour and depth for queue ≥ 3000 (`scene_color_at`, `scene_world_pos`, …).
- Materials: `shader: "Assets/Shaders/X.shader"`, values under `properties` in the `.mat`. The Inspector builds
  sliders, colours, vectors and texture slots from the header, and shows the render queue and compile status.
- Hot reload on save or file change, and live while typing in the Code Editor. naga errors (with user line numbers)
  appear in the Console (clickable) and as Code Editor squiggles. Broken shaders draw magenta. The Code Editor
  highlights WGSL and folds braces.
- Assets ▸ Create ▸ Shader ▸ Surface Shader / Unlit Shader, and Material from Shader.
- Instancing: custom materials batch by pipeline + material group + mesh, like built-ins.
- Water: a `Water Surface` component (Ocean / Lake / River) from GameObject ▸ Water. The ocean is a camera-following
  9-ring clipmap with a horizon skirt; lakes and rivers are scaled grids.
- Waves: 8 Gerstner waves (swell + long wind waves) and a 256² GPU FFT ocean (JONSWAP or Phillips). Controls: wind
  speed and direction, wave height, choppiness, swell height, length and direction, ripples, river flow, plus a
  Calm ↔ Stormy slider. Foam comes from the FFT Jacobian (accumulated and fading), and the displacement and slope
  textures are mipmapped.
- Shading: Fresnel sky reflection plus screen-space reflections; refraction with depth absorption (shallow↔deep,
  clarity); crest SSS; GGX sun glitter; shore foam from scene depth and crest foam; procedural caustics on
  underwater geometry; the sun's shadow and fog. Underwater: fog, tint, wobble and caustics, with Snell's window on
  the underside of the surface.
- Interaction: `Water.GetHeight(x, z)` / `(Vector3)` in Lua and `water::height_at` in Rust use the same
  Gerstner + FFT low modes as the GPU. A `Floater` component (8 corner probes, water drag relative to the flow)
  makes Rigidbodies bob.
- Inspector: shape, presets (Tropical, Ocean, Stormy, Lake, Murky River), Calm ↔ Stormy, and collapsible Waves /
  Color & Clarity / Foam / Reflections & Refraction / Caustics / Underwater sections with tooltips.
- Public API for world generation: `water::spawn_water`, `water::height_at`, `WaterSurface`, `Floater`.
- Samples: `Scenes/WaterDemo.scene` (sunset ocean, island, floating crates, hologram beacon, orbiting camera) and
  `Scenes/ShaderDemo.scene` (Hologram, Wobble, Scroll Gradient, Force Field), generated by `sample_water`.
- Tests: header parsing and errors, uniform layout, wrapping + naga validation of the templates, user-line error
  mapping, a headless render of a custom colour, the magenta fallback and recompiling; spectrum Hs and Hermitian
  symmetry, the height-query inverse, height queries over lakes and oceans, buoyancy (floats, sinks without a
  Floater, bobs in a storm), Lua `Water.GetHeight`, a headless ocean render, clipmap coverage, WGSL lexer and
  squiggles, and the sample generator check plus a headless play test.

### Water GPU cost (RTX 4060, Vulkan, 1920×1080, release, `water_benchmark`)
- Scene with one sphere and a sun: 0.55 ms without water, 1.70 ms with the ocean, so the ocean costs about **1.15 ms**
  (FFT + mips, the scene copy, the 57k-triangle clipmap, SSR and shading; whole-render GPU timer, median of 60 frames).
- Screenshots: docs/screenshots/round8/water_sunset_island.png, water_island_day_view.png, shader_demo.png.

### Not matching Unity / HDRP yet
- No ShaderLab or Shader Graph, no multiple passes or keywords/variants, no per-shader shadow-caster pass
  (displaced or blended custom shaders cast the plain mesh shadow), no geometry/tessellation stages.
- Water has no reflection-probe reflections, no water excluder or deformers, no current maps, no wake or
  ripple simulation, no cascaded FFT (one 256² tile sampled at three scales), no volumetric underwater light
  shafts, and no water masks. All surfaces share the first surface's FFT. CPU heights leave out the finest
  ripples (32² modes).
- Buoyancy uses box probes, not mesh volume integration.

## Light halos and lens flares ✅

### Done
- Light ▸ Halo (point and spot lights): Enabled, Size (m), Intensity, Color (the light's, or Custom), Softness and
  Fog Response, in a collapsible Inspector section. Drawn as sphere-facing additive HDR sprites into both the scene
  and the bloom (emission) image, so bloom spreads them.
- Soft occlusion: the vertex shader reads the main depth at 13 taps around each light, so halos and flares fade as
  objects pass in front (no popping); a bias keeps the lamp's own head and bulb from hiding it, and halos also fade
  per pixel against geometry just in front of them.
- Fog-aware: extinction at the light (Fog override height fog, Fog Volumes, Lighting fog) widens the halo up to 1.5×
  and brightens it from a subtle 0.35× in clear air to 1.75× in thick fog; transmittance to the eye dims it.
- Spot lights glow only when the camera looks into the cone (smooth fade at the edge).
- Lens Flare component: procedural glow, ring, horizontal streak and up to 8 tinted ghosts on the line through the
  screen centre, occlusion-tested, fading at screen edges; works on directional lights too (sun flares).
- Quality: Project Settings ▸ Quality ▸ Lens Flares (off on Low); halos always draw. Scene view: glows follow the
  effects / Post Processing / Lighting toggles and never draw in Wireframe.
- Sample: `Scenes/LightGlowDemo.scene` (foggy night street, warm and cool lamps, doorway spot, floodlight with a
  lens flare, ground mist), generated by `sample_light_glow`.
- Tests: halo serialisation, fog extinction / fogginess, spot facing, fog widening, culling, flare elements, Quality
  gating, edge fades, sun anchoring, scene gathering; headless GPU checks of the halo, full and partial occlusion,
  the lamp-shade bias, fog spreading and flare ghosts; the sample generator check.

### Glow GPU cost (RTX 4060, Vulkan, 1920×1080, `glow_benchmark`, whole-render GPU timer, median of 60 frames)
- 0.49 ms without glows, 0.65 ms with 64 overlapping 1.5 m halos plus one 11-element lens flare: about **0.16 ms**,
  including the main pass now keeping its depth.
- Screenshots: docs/screenshots/round9/glow_street_game.png (LightGlowDemo Game view), glow_inspector_halo.png
  (Light Inspector with the Halo section), glow_behind_floodlight.png (behind the floodlight: no spot halo or flare).

### Not matching Unity / HDRP yet
- No flare asset or element editor (procedural elements only), no textured flares or halos, no per-element
  chromatic aberration; flares don't read the light's cookie or shape.
- Halos read the fog density on the CPU from the fog settings (noise in the fog is not included).
- Occlusion uses the main pass depth: transparent objects and particles don't occlude glows.

## Round 9: HDRI skies ✅

### Done
- Import: `.hdr` (Radiance RGBE) and `.exr` equirectangular images (`image` crate `hdr` + `exr` features) decode to
  linear float (`hdri::HdrImage`, ≤ 4096 wide). The Project window shows them as textures with a tonemapped
  thumbnail (auto exposure + Reinhard). Our own Radiance writer (`hdri::rgbe`, RLE scanlines).
- Lighting ▸ Environment ▸ Skybox = HDRI: image slot (picker or drag from the Project window) with a preview,
  Rotation (°), Exposure (EV), Tint and **Align Sun to HDRI** (turns the main Directional Light to the brightest
  point). Dropping an HDR file on the Scene view makes it the sky (undoable). Stored by asset path in the scene
  (`lighting.skybox.hdri`, omitted while default: older scenes load and save unchanged).
- Rendering (`render::hdri_sky`): equirect → 512² background cube with a box mip chain and a 128² specular cube
  with seven GGX levels (filtered importance sampling, 512 samples) on the GPU, once per file; diffuse SH9 on the
  CPU. Bound in the view group, so the skybox, meshes, terrain, grass, water and custom shaders all use it.
  Rotation / exposure / tint apply at lookup (no rebuild).
- Probes: the HDRI fills whatever the reflection probes leave (unbaked probes fall back to it); baked and Realtime
  probes capture it through the skybox.
- Fog and water: volumetric fog's ambient and the water's ambient come from the HDRI's SH; ocean reflections
  sample the prefiltered sky cube at the water's smoothness.
- Sample: `Assets/HDRI/Sunset Clouds.hdr` (procedural 2048 × 1024 sunset: gradient, sun disc, glow, fbm clouds),
  `Scenes/HdriSkyDemo.scene` (chrome → rough steel, glossy → matte white and gold spheres), generated by
  `sample_hdri`.
- Tests: equirect mapping, sampling, RGBE round trip through the image decoder, SH of a constant sky, brightest
  point, rotated SH, sun Euler angles, previews, settings round trip and old files, headless GPU checks (background
  faces and Rotation, a probe capture of the HDRI, diffuse and chrome spheres lit only by the HDRI, top vs bottom),
  Lighting window rows and the Scene-view drop with undo, the sample generator check.

### Cost (RTX 4060, Vulkan, release test build, `hdri_sky::gpu_tests`)
- 2048 × 1024 HDRI → cubes: **28 ms** once per file (CPU half-float conversion + upload + GPU passes + wait);
  512 × 256: 7 ms. Per frame: one extra cube sample in the skybox and two cube samples + SH9 per lit pixel instead
  of the analytic sky (no measurable change). Memory ≈ 18 MB per HDRI (cubes; the equirect upload is freed).
- Screenshots: docs/screenshots/round9/hdri_game.png, hdri_lighting_window.png.

### Not matching Unity / HDRP yet
- No HDRI importer settings (max size, cube layout), no cubemap-cross / six-face images, no physically based sky
  units (lux), no sun extraction into a separate light-plus-sky split (the sun stays in the image and is also a
  Directional Light), one HDRI cached on the GPU at a time (views with different HDRIs rebuild).

## Rectangle area lights ✅

### Done
- Light ▸ Type ▸ Rectangle with Width, Height, Range, Color, Intensity (the surface's radiance), Two Sided and
  Show Emitter; GameObject ▸ Light ▸ Rectangle Light creates a ceiling panel shining down.
- Shading with linearly transformed cosines: GGX specular and Lambert diffuse integrated over the rectangle
  (horizon-clipped polygon, exact edge integrals), from a 64 × 64 LTC table fitted by our own code
  (`render/area/ltc_fit.rs`, Nelder–Mead on the cubed lobe error; `write_ltc_table` regenerates
  `ltc_ggx.bin`). One-sided lights are dark behind; two-sided ones light both ways.
- Works everywhere lit: meshes, terrain, grass and custom `.shader` materials (shared `shade()`), and water
  (reflected glints plus light into the water body).
- Shadows: one perspective shadow map from the centre (120°), PCSS penumbra from the rectangle's size; contact
  shadows towards the centre.
- Volumetric fog: rectangles scatter into the froxels as an emitter at the centre with the rectangle's solid
  angle, shadowed by its map.
- Show Emitter: the rectangle drawn as an emissive quad (bloom picks it up) in the Scene and Game views, the
  player and probe captures.
- Editor: Inspector rows only for rectangles; gizmo outline + corner rays + forward arrow; draggable edge-midpoint
  handles resize Width / Height symmetrically (undoable).
- Sample: `Scenes/AreaLightDemo.scene` (generated by `sample_area_lights`): a dim room with a ceiling strip, a tall
  wall panel and a two-sided square over glossy and rough surfaces.
- Tests: Nelder–Mead, table cells re-fitted from their neighbours, albedo sanity, diffuse vs brute-force form
  factor, LTC specular vs brute-force GGX (< 12 %), one-sided / horizon clipping, emitter orientation, a headless
  render (front lit, back dark, two-sided, emitter visible), the sample generator check, shader validation.

### GPU cost (`area_light_benchmark`, release, 1920 × 1080, floor + sphere, whole-render GPU timer, median of 60)
- RTX 4060, Vulkan, three runs: no local lights 0.70–0.73 ms, 8 point lights 0.97–1.04 ms, 8 rectangle lights
  2.41–2.60 ms, 8 rectangles with 4 soft-shadowed 2.88–3.49 ms. A full-screen rectangle light costs about
  **0.22 ms** at 1080p (≈ 5× a point light: two polygon integrals with clipping); a soft shadow adds 0.1–0.2 ms.
- The first shader version clipped into an indexed local array and cost about 1 ms per light (spills to slow
  memory); clipping edge by edge with plain values brought it to the numbers above (decision 463).
- Screenshots: docs/screenshots/round9/arealight_game.png (Game view), arealight_scene_gizmo.png (Scene view,
  Wall Panel selected: edge handles and Inspector rows).

### Not matching HDRP yet
- No disc / tube / line area lights, no textured (cookie) rectangles, no barn doors.
- Shadows are a single 120° perspective map from the centre (no shadows far off-axis, no back-side shadows for
  two-sided lights); fog treats the rectangle as a point emitter.
- Intensity is radiance in engine units, not lumens / nits; no anisotropic GGX in the table.

## Volumetric clouds ✅

### Done
- Post-process Volume ▸ Volumetric Clouds: Enable, Preset (Sparse / Cloudy / Overcast / Stormy, "Custom" once
  edited), Shape (Coverage, Density, Fluffy ↔ Sharp), Layer (Altitude, Thickness), Wind (Direction, Speed,
  animated), Lighting (Silver Lining, Ambient, Cast Shadows, Shadow Opacity). Saved only when touched.
- Raymarched layer on a curved planet with GPU-generated tiling noise (128³ Perlin-Worley base, 32³ Worley
  detail, 512² weather map) and a CPU void-and-cluster 64² blue-noise tile.
- Lighting: sun march with multiple-scattering octaves, two-lobe phase (silver lining), powder edges, sky ambient
  by height and darkened by the cloud above (dark storm bases), sunset-warmed sun colour, aerial perspective into
  the horizon haze.
- Reduced-resolution trace (¼ or ½ per axis) with blue-noise jitter and temporal reprojection (variance-clipped
  history); depth-independent, so bilinear upsampling has no edge artefacts.
- Clouds are folded into the fog functions every forward shader uses: behind the fog in the sky, hidden by
  geometry in front of them (mountains in cloud and views from above work), sun disc and bloom dimmed behind them.
- Cloud shadow map (8–16 km around the camera, texel-snapped) dims the sun on meshes, terrain, grass and water,
  in the froxel fog (god rays through cloud gaps) and the analytic fog; sun shafts only see the sky between
  clouds.
- Quality ▸ Volumetric Clouds tier (Off / Low / Medium / High) in Project Settings; the default levels use Low,
  Medium, Medium, High.
- Scene view effects dropdown: Clouds toggle next to Fog.
- Sample: `Scenes/CloudsDemo.scene` (generated by `sample_clouds`): meadow valley, low sun ahead, Cloudy clouds,
  haze and sun shafts.
- Tests: compute shader validation (and every forward shader through `COMMON`), uniform layouts, tiers, wind and
  layer clamping, shadow-map snapping, blue-noise permutation and spectrum, presets round trip, old files load,
  and headless GPU renders: overcast whitens the sky, the Off tier draws nothing, geometry in front hides the
  clouds, cloud shadows darken the ground (and don't with Cast Shadows off). Sample generator check and play test.

### Clouds GPU cost (RTX 4060, Vulkan, 1920×1080, release, `clouds_benchmark`)
- Added to a 0.59 ms frame (sphere + ground, camera 12° up): Cloudy +0.47 / +0.72 / +2.33 ms and Overcast
  +0.19 / +0.30 / +1.17 ms on Low / Medium / High (whole-render GPU timer, median of 60 frames). Medium meets the
  < 2 ms target.
- Screenshots: docs/screenshots/round9/clouds_demo.png, clouds_demo_game.png, clouds_presets.png (Sparse,
  Cloudy, Overcast, Stormy), clouds_shadows_aerial.png.

### Not matching Unity / HDRP yet
- One layer only (no second high cirrus layer, no 2D cloud layer or user-painted cloud maps), no local cloud
  volumes, no curl-noise distortion, no lightning.
- Cloud shadows cover 8–16 km around the camera; the ambient light on the ground doesn't drop under overcast.
- Clouds aren't captured by reflection probes, and their drift uses the renderer clock (not Play mode time).
- Every directional light is shaded by the cloud shadows (they are traced along the main sun only).

## Round 9 merge ✅

### Done
- Light halos / lens flares, HDRI skies, rectangle area lights and volumetric clouds merged into main (decisions
  432–478). View group: fog 1–3, HDRI sky 4–7, clouds 8–10; LTC table at mesh group 1 binding 7.
- Clouds under an HDRI sky take their ambient from the HDRI's SH; cloud shadows stay on directional lights only.
- Rectangle lights draw no halo (their emitter quad is their shape); the Scene view effects dropdown lists Skybox,
  Fog, Clouds, Halos & Flares and Post Processing.
- Screenshots of the merged build (Game view, release): docs/screenshots/round9/merged_LightGlowDemo.png,
  merged_HdriSkyDemo.png, merged_AreaLightDemo.png, merged_CloudsDemo.png and merged_clouds_hdri.png (CloudsDemo
  with the Sunset Clouds HDRI as its skybox, from a temporary scene copy).

### Not done yet
- Reflections of an HDRI sky ignore the volumetric clouds: under clouds the HDRI's sun still glints off the terrain
  (a streak towards the sun in merged_clouds_hdri.png). Cloud-aware sky reflections / probes are a follow-up.
- Clouds don't hide the HDRI's own baked-in sun or clouds; pairing a clear-sky HDRI with volumetric clouds looks best.

## Round 10 — Multi-scene editing ✅

### Done
- Several scenes open at once in one world; each object carries its scene (decisions 479–489). The active scene
  provides lighting and navigation settings and receives new objects; all loaded scenes render and simulate
  together.
- Hierarchy: a header per scene (bold = active, `*` = unsaved, greyed = not loaded) with Set Active Scene, Save
  Scene / Save Scene As, Select All, Unload / Load Scene, Remove Scene and Close Others; drag objects onto a header
  (or between another scene's rows) to move them; drop a `.scene` to open it additively.
- Project window ▸ Open Scene / Open Scene Additive on scene assets.
- File ▸ Save saves every changed scene; per-scene dirty tracking by content; the save prompt lists the affected
  scenes for New / Open / Quit / Unload / Remove / Close Others.
- Cross-scene references are warned about and saved as None.
- The open set (and active scene) is remembered in `Library/EditorSettings.json` and restored on launch.
- Scripts: `SceneManager.LoadScene(name, LoadSceneMode.Additive)`, `UnloadSceneAsync`, `GetActiveScene`,
  `SetActiveScene`, `GetSceneByName`, `GetSceneCount`; works in Play mode and in `misty_player` builds.
- Sample: `MultiSceneLevel` + `MultiSceneLighting` (generated by `sample_multiscene`), both in the sample's Build
  Settings; screenshots: `MISTY_SCENE` takes a comma list, `MISTY_ACTIVE_SCENE` picks the active one.
- Tests: engine (tagging, settings swap, per-scene save, move between scenes, unload/remove, cross-scene refs, Lua
  requests), editor (dirty per scene, prompts, unload/reload/close others, open-set restore, undoable moves, header
  UI), player (additive load plays, lighting follows SetActiveScene, unload), sample generator and a play test of
  both sample scenes together.

### Not done yet
- No async loading progress (loads happen between frames), no `SceneManager.MergeScenes` /
  `MoveGameObjectToScene`.
- Navigation uses only the nav mesh of the scene active when Play started; baked reflection probes and nav data
  are stored next to the active scene only.
- Undo history is cleared when scenes are loaded, unloaded or removed.

## Round 10 — Undo History ✅

### Done
- Edit ▸ Undo History (Ctrl+H), a dockable on-demand window: every step by name, newest first, the current step
  highlighted, redoable steps greyed below, "Initial State" at the bottom. Click a row to undo/redo to it; search
  box; Clear History; footer with step count and approximate memory.
- Named steps everywhere: "Change Light Intensity", "Change Fog Density", "Move / Rotate / Scale Cube",
  "Create Sphere", "Delete 3 Objects", "Rename Cube", "Reparent Cube", "Paint Terrain", "Smooth Terrain",
  "Enable Fog Volume"… Inspector edits are named from the field that changed (JSON diff).
- Audit: Inspector components that changed values without reporting it now make undo steps (value comparison in
  `component_section`), material asset edits undo (and re-save the .mat), terrain + scene edits in one interaction
  share a step, Script field and enable toggles, World Builder, Navigation and Lighting edits are named.
- History cap: 200 steps / ~512 MB; terrain steps store only the changed 32×32 tiles (heights, splat, details).
- Selection is restored with each step but never a step of its own.
- `MISTY_UNDO_DEMO=1` for screenshots; docs/screenshots/round10/undohistory_window.png.
- Tests: jump to any step and back, redo branch dropped after a new step, grouping of a drag into one step,
  count and byte caps, Clear History, material undo, terrain tile diff / swap / resize, brush-stroke undo size,
  step naming (field diff, humanize, selection names), window rows / click-to-jump / search / clear.

### Not matching Unity yet
- Project Settings, texture import settings and build settings have no undo (saved immediately).
- Steps are whole-scene snapshots (cheap for typical scenes; the memory estimate is approximate).
- No "group" collapse of related steps in the window; a jump replays steps one by one.

## Round 10: Presets ✅

### Done
- `.preset` assets (`misty_engine::preset`): a component type or asset kind (Material, Texture Importer) plus its
  serialised values and a list of excluded fields. Applying sets every included field (fields the preset leaves
  out reset to their defaults) and leaves excluded ones alone (decisions 500–502).
- ⚙ preset button next to ⋮ in every component header (and on material and texture import Inspectors) opens the
  Select Preset window: None (back to the values from when it opened), the project's matching presets, and
  "Save current to…" (`Assets/Presets/<Name>.preset`). Picks apply to every Inspector target in one undo step.
- `.preset` Inspector: what it targets, Apply to Selection (or to a chosen material / texture), "Default for new …",
  and every field with an include checkbox and a generic editor (numbers, toggles, colours, vectors, enum
  dropdowns, nested foldouts). Valid edits save right away.
- Project Settings ▸ Preset Manager: default preset per type, applied on Add Component, GameObject ▸ Create,
  Reset and Assets ▸ Create ▸ Material (a Light's type from the Create menu is kept).
- Copy Component / Paste Component Values / Paste Component As New / Reset (existing) work across the selection;
  Reset now goes to the default preset when there is one.
- Sample presets (`sample_presets.rs`): Warm Street Lamp, Stormy Sky (clouds + fog only), Bouncy (sphere collider
  surface only), Pixel Art (texture import settings).
- Tests: engine round trip / defaults reset / exclusions / asset presets / field walker; editor apply to every
  target + undo, None restore, default preset on add / create / reset / new material, enum discovery, sample
  generator check and application.
- Screenshots: docs/screenshots/round10/presets_picker_light.png, presets_inspector.png,
  presets_manager.png.

### Not matching Unity yet
- Exclusions are per top-level field only; no filters in the Preset Manager (one default per type).
- Defaults don't apply to UI elements or terrains made from the GameObject menu, nor to imported textures.
- Material / texture preset applies aren't scene undo steps; the preset Inspector edits values generically (not
  with each component's own editor) and can't turn a field that is at its default into a stored value.

## Round 10: Global Search (Ctrl+K) ✅

### Done
- Ctrl+K opens a floating search palette near the top of the window (not a dock tab): one box, results grouped
  under small provider headers with icons, matched characters highlighted, Up/Down/Enter/Esc, Alt+Enter or
  right-click for more actions, recent searches when the box is empty (decisions 514–524).
- Providers: Hierarchy objects (name + `t:Light`, `t:Camera`, any component, `tag:`, `layer:`), Project assets
  (name + the Project window's `t:` types), Menu commands ("Create Cube", "Add Rigidbody", "Lighting",
  "Project Settings", layouts, recent scenes), Settings pages (Project Settings pages, Preferences; blurbs as
  keywords), Help (headings of the guides in docs/) and a calculator (`=2*3`).
- Prefixes `h:` `p:` `m:` `s:` limit the search to one provider (and list everything it has when nothing else is
  typed).
- Enter: select and frame an object / ping an asset in the Project window / run a menu command / open a settings
  page / open a guide / copy a calculation. Secondary: Select, Copy name, Open (scene, script, folder), Show in
  Explorer, Copy path.
- Shared matching (`search::fuzzy`, `search::query`, `search::scene_search`): the Hierarchy box keeps substring
  matching but now also understands `t:` / `tag:` / `layer:`; the Project box parses and matches through the same
  code.
- Asset index built on a background thread, then kept fresh by checking a few folder times per frame (no hitch
  with thousands of assets); results cached per query.
- `MISTY_SEARCH=<text>` opens the palette for screenshots.
- Screenshots: docs/screenshots/round10/search_*.png.

### Not done yet
- The menu table is separate from the menu bar's inline egui menus (kept in sync by hand; see decision 520).
- No search inside asset contents, component values or scripts; no saved searches or search-to-Hierarchy view.
- Help opens the Markdown guide in the system viewer, not at the heading.

## Round 10 — Asset GUIDs and .meta files ✅

### Done
- `misty_engine::asset_db`: random 128-bit GUIDs, `<asset>.meta` sidecars for every file and folder under `Assets/`
  (GUID first, texture import settings after it), and a `GuidDatabase` mapping GUID ↔ path, built by scanning on
  project open (missing metas created, copied GUIDs split) and kept in step by every asset refresh plus a 2 s poll.
- Scenes, prefabs, materials, terrains, clips, controllers, mixers, biomes and Build Settings save an
  `"asset_guids"` table next to their path references; loading follows a renamed or moved asset by GUID and falls
  back to the path. Old path-only files load unchanged and are upgraded when saved.
- Project window: rename / move carry the meta along, duplicate gives copies new GUIDs, delete trashes the meta.
  Files saved with GUIDs are no longer rewritten on rename. New Reimport and Find References In Scene entries.
- Inspector: every asset shows its GUID (small, selectable, copy button) under its path.
- Builds pack `AssetGuids.json`; the player resolves references with it.
- Sample assets have committed `.meta` files (except the six protected user files); generated samples stay
  path-only so their generator tests are unchanged (decision 532).
- Decisions 527–536.

### Not done yet
- No OS file-system watcher (polling only, in Edit mode); a meta changed outside the editor (e.g. a git checkout
  changing a GUID) is only picked up by the next refresh.
- Orphan `.meta` files (asset deleted outside the editor) are left on disk.
- References inside Lua scripts and `.obj`/`.gltf` files stay plain paths.

## Fix: fog over water ✅

### Done
- Water surfaces (ocean / lake / river) now take the volumetric / height fog of the Post-process Volume (froxels,
  local Fog Volumes, the clouds' distant haze) through `vfog_eval`, as meshes do; before, only the Lighting
  window's fog reached them, so distant ocean stayed sharp under volumetric fog.
- Fog is applied once: to the surface's own light, with its in-scattering scaled by the part the surface covers;
  the refracted scene (already fogged in the scene copy) is added after (decision 538).
- Camera under water: air fog (Lighting and volumetric) is off for that view, so meshes under water are no longer
  fogged twice; the underwater fog / tint is unchanged (decision 540).
- `apply_fog` for custom unlit shaders now includes the volumetric fog. Terrain, grass, particles, surface-model
  custom shaders and the built-in transparent modes already applied both.
- GPU tests (`render::water::gpu_tests`): far water under dense Lighting fog and under volumetric fog matches a
  fully fogged reference while near water stays mostly clear; air fog changes nothing under water; WaterDemo
  looking out to sea with fog turned up.
- Screenshots (Game view, release): docs/screenshots/fixes/water_fog_volumetric_before.png / _after.png
  (WaterDemo + a Post-process Volume with height fog: before, the island fogged while the ocean stayed clear to the
  horizon) and water_fog_lighting_before.png / _after.png (WaterDemo + exponential Lighting fog, which already
  reached the water; unchanged apart from the refraction no longer being fogged twice). Both from temporary scene
  copies that are not committed.

### Not done yet
- Screen-space reflection hits keep their eye → hit fog (slightly over-fogged reflections).
- The world above seen from under water (Snell's window) gets no air fog.
- The island's volumetric shadow on the fog over the near water shows the froxel grid's steps (the same on any
  mesh; the wide flat sea makes it easier to see).

## Sharp sun shafts ✅

### Done
- Fog ▸ Sun Shafts Quality: Off (froxels, the old look) / Sharp (default). Sharp takes the main sun out of the
  froxels and raymarches it per pixel (half resolution, quarter on Low) through the sun cascades with hard 2×2 PCF
  and the cloud shadows, using the froxels' density, phase and colour; temporal accumulation (clamped while the
  view moves) and a depth-aware upsample add it onto the HDR image. New module `render/volumetrics/sharp_shafts.rs`
  with `sharp_common.wgsl`, `sharp_shafts.wgsl`, `sharp_composite.wgsl`; the density / phase / shadow code moved
  from froxel_light.wgsl into `froxel_medium.wgsl`, shared by both (decisions 542–548).
- Quality ▸ Sharp Sun Shafts tier (Low level Off, Medium/High Medium, Ultra High) in Project Settings ▸ Quality.
- The screen-space override is now "Sun Shafts (Screen)".
- Shared forward apply path untouched: fog_apply.wgsl and water.wgsl are unchanged (water and transparents get
  the shafts from the composite after the main pass).
- Tests: shader validation, uniform sizes, main-sun slot / Off packing, override + quality defaults and old files;
  GPU: a 25 cm hole in a roof gives a 7 px beam with black fog beside it (froxels: a 17 px smear), Sharp matches
  the froxel brightness in unshadowed fog (206.9 vs 205.4), Off in the override or the Quality level is the
  froxel image pixel for pixel.

### GPU cost (RTX 4060, Vulkan, 1920×1080, release, `sharp_shafts_benchmark`, VolumetricFogDemo)
- Whole frame 1.61 ms with Sun Shafts Off; Low +0.22 ms, Medium +0.73 ms, High +1.30 ms (fog noise on; noise off:
  +0.21 / +0.67 / +1.18 ms). Medium meets the ≤ 1 ms target.
- Screenshots (Scene view, release build, shadow side of the ruined wall): docs/screenshots/fixes/
  godrays_wall_before.png / godrays_wall_after.png, godrays_forest_before.png / godrays_forest_after.png,
  godrays_side_before.png / godrays_side_after.png and godrays_wall_detail_before_after.png ("before" = Sun
  Shafts Quality Off from a temporary scene copy, identical to the previous build).

### Not done yet
- Only the main sun is marched; spot / point lights still light the froxels (soft shafts).
- Transparent surfaces that don't write depth don't attenuate the shafts behind them.
- A moving view clamps the history, so very thin shafts (a few cm at tens of metres) flicker slightly while moving.
- Shafts end at the Volumetric Distance; past it the analytic fog adds unshadowed sun light as before.

## Round 10 merge ✅

### Done
- Merged in order: multi-scene editing, Undo History, presets, global search, asset GUIDs, water fog fix, sharp
  sun shafts. Decisions renumbered into one sequence (479–548); references in docs updated.
- Integration: undo keeps the active scene and scene file paths (decision 499); a material preset pick is one
  undo step (513); search finds objects in every loaded scene and offers Undo History, additive opens, scene
  header commands, Apply Preset, Open Scene Additive, Reimport and Find References (525, 537); per-scene saves
  and `.preset` files carry the GUID table (537); engine tests keep a per-thread game clock (526).
- Sharp sun shafts follow the water fog fix under water: the march only runs with the froxel fog, which is off
  while the camera is under water; water writes depth, so the composite puts the shafts over it.
- `.meta` files for the round's new sample assets (MultiSceneLevel / MultiSceneLighting scenes,
  AdditiveLoader.lua, the Presets folder and its four presets).
- Screenshots (release build): docs/screenshots/round10/merged_multiscene_hierarchy.png,
  merged_undo_history.png, merged_search_palette.png (hits name their scene), merged_preset_picker.png,
  merged_godrays.png (VolumetricFogDemo, shadow side of the wall), merged_water_fog.png (WaterDemo opened with
  VolumetricFogDemo active: the wall's shaft shadows reach across the water).

### Not done yet
- The undo demo (`MISTY_UNDO_DEMO`) has no Move To Scene step.
- sample_clouds / sample_materials generator checks fail in this working copy only, because of the uncommitted
  edits to CloudsDemo.scene / CloudsDemo.terrain and Ghost.mat.

## v0.0.1: Misty Hub and templates ✅

### Done
- `misty` with no arguments opens **Misty Hub** (`misty <project>` still opens the editor directly;
  `misty --new <Template> <folder>` creates and opens a project). The Hub hands the same window over to the editor.
- Sidebar: logo + wordmark, Projects / Learn / Settings, version footer (v0.0.1 from the Cargo version).
- Projects: recent list with thumbnails, last-opened times and search; New project and Open… (folder picker);
  per-row ⋯ menu with Show in Explorer and Remove from list; missing folders greyed with Locate….
- New project page: template cards with previews and descriptions, name + location (Browse…), validation, Create
  on a worker thread, then the project opens on its starting scene.
- Templates generated in code: Empty, Standard (floating platform), Ocean (platform + ocean, post-processing on),
  Landscape (World Builder terrain, volumetric clouds and fog, sun shafts, AO, bloom). Each writes a full project
  with Build Settings listing its scene; thumbnails are rendered headlessly.
- Learn page listing a tutorial registry (`hub::learn::TUTORIALS`, "Coming soon" until tutorials are added).
- Settings: default projects folder, Dark / Light theme, clear the recent list; stored in
  `%APPDATA%\MistyEngine\hub.json`.
- Screenshot hooks: `MISTY_HUB_PAGE=projects|new|learn|settings`, `MISTY_HUB_TEMPLATE`, `MISTY_HUB_CONFIG`,
  `MISTY_HUB_OPEN`. Screenshots: docs/screenshots/v001/hub_*.png and template_*.png.
- Tests: hub.json load/save and recent-list operations, name validation, Hub pages (kittest: list, missing
  project, open, New project validation and creation, Learn, Settings), every template's files/scene/objects/post
  volume, the editor opening a template's scene, and a headless GPU render of every template.

### Not done yet
- No project thumbnail is rendered when the editor closes (only template-created projects have one).
- The Learn page has no tutorials yet; no project upgrade or "open with another version".
- The Documents folder isn't resolved through the Windows known-folder API (OneDrive redirection is ignored).

## v0.0.1: Tutorials ✅

### Done
- Help ▸ Tutorials (also "Tutorials" in Ctrl+K) opens a small window: title, short description, minutes and
  a ✓ for finished ones, with Start / Again.
- A running tutorial shows a floating step card (Step N of M, progress bar, title, 1–3 sentences, Back /
  Next / Skip / Exit) beside the element it talks about, with a soft pulsing outline around it and a light
  dim elsewhere. Clicks go through to the editor everywhere but the card.
- Steps finish on their own when their condition is met (✓ Done!), Next always works.
- Four tutorials (decisions 526–537): "Make it beautiful: Post-processing" (12 steps), "Light shafts and
  god rays" (7), "Sculpt and paint a terrain" (12), "Generate a world with World Builder" (12). Each opens a
  fresh Untitled scene with what it needs.
- Targets: Hierarchy rows, Inspector component headers, Post-process Volume sections and fields, Light,
  Terrain tools / layers / rules / Generate, World Builder sections and fields, menu bar entries, Scene view
  effects toggle, Play button and whole panels.
- `tutorials::list()` for the Hub's Learn page; progress in the per-user `tutorials.json`;
  `MISTY_TUTORIAL=<id>[:<step>]` to launch into a tutorial or capture a step.
- Tests: data loads and is complete, Hub list, navigation and saved progress, change-only auto-advance and
  `undo:` conditions, every tutorial run end to end in kittest through the real menu bar, toolbar and
  docked panels (every target drawn, every condition met by its demo), real clicks advancing steps, menu
  targets opened by clicking.
- Screenshots: docs/screenshots/v001/tutorial_*.png.

### Not done yet
- No "resume where I left off"; Exit forgets the step.
- Targets inside a collapsed section fall back to the section header until the user opens it.
- Tutorial text is English only.

## v0.0.1: Hub and tutorials merged ✅

### Done
- Merged `v001-hub` and `v001-tutorials` (decisions renumbered 549–562 and 563–575).
- The Hub's Learn page lists the four built-in tutorials with their time and a ✓ Done mark from the tutorial
  progress; Start opens a recent project you pick (or the "Tutorial Playground" project, made from the Standard
  template in the projects folder) and the editor starts the tutorial once it has loaded.
- Hub and tutorial progress share `%APPDATA%\MistyEngine\` (the old `Misty Engine\Editor\tutorials.json` is still
  read until the new one exists).
- File ▸ Open Misty Hub / New Project... / Open Project... return to the Hub (asking to save first).
- Fixes found while merging: the post-processing tutorial follows the "Sun Shafts (Screen)" rename; the water
  GPU tests freeze the wave clock (one flaked in a full run).

### Not done yet
- Returning to the Hub does not ask about scripts changed in the Code Editor; it refuses with a Console warning.

## v0.0.1: Branding ✅

### Done
- The designer's official art in the editor (decisions 549–559): `crates/misty_editor/assets/branding/` (v3 logo,
  icon crop, small logo, `misty.ico`) and `assets/icons/` (hand, move, rotate, scale, grid, filter, folder, camera,
  character controller, sky volume, sun light), embedded with `include_bytes!`.
- New module `branding/`: `icons.rs` (alpha-mask icons tinted with the theme's text / accent colour, per-pixel-size
  texture cache, icon buttons), `about.rs` (Help ▸ About Misty Engine), `splash.rs` (startup splash while the
  project opens), `mod.rs` (`logo_texture(ctx)`, `small_logo_texture(ctx)`, `window_icon()`, ICO check).
- Scene view: the tool strip is now a horizontal row like the mockup (View / Move / Rotate / Scale icons, Rect,
  Transform, divider, grid toggle); hotkeys and tooltips unchanged.
- Camera / Character Controller / Post-process Volume / Directional Light icons in the Hierarchy, the Inspector
  (object icon and component headers) and the Scene view billboards; folder icons in the Project window; a filter
  button with a "Search by Type" menu next to the Project search.
- misty.exe and misty_player.exe carry the Misty icon (16–256 px) and version info through `winresource`; game
  builds still replace the player's icon with the project's own. The window / taskbar icon is set at runtime.
- Main toolbar mark: the small logo + "Misty".
- Startup splash (logo, name, "Opening <project>…") while a project opens, skipped when the last open of that
  project took under 400 ms (kept in `Library/EditorStartup.txt`); the Console logs the open time.
- Help ▸ About Misty Engine: logo, "Misty Engine v0.1.0", credits line, license placeholder.
- For the Misty Hub (v001-hub): `branding::logo_texture(ctx)` / `small_logo_texture(ctx)` give the logo as a
  cached `TextureHandle`; `branding::window_icon()`, `branding::PRODUCT`, `branding::version()`.
- Capture env: `MISTY_ABOUT=1` (About window), `MISTY_SPLASH=1` (the splash), `MISTY_THEME=light` (egui light
  visuals, for checking icon tints).
- Tests: ICO matches the logos (7 sizes), logo / icon decoding, per-pixel-size texture cache, theme tints, About
  window, splash skip rule, Project type-filter words, and a game build's icon replacing the player's Misty icon
  (one neutral-language icon group before and after).
- Screenshots (release build): docs/screenshots/v001/branding_editor.png (tool row, Directional Light in the
  Hierarchy / Inspector, folders, filter button), branding_inspector_camera.png, branding_character_controller.png,
  branding_post_volume.png, branding_about.png, branding_splash.png, branding_light_theme.png (light visuals),
  branding_exe_icon.png (misty.ico's 7 sizes on dark and light, plus the icons Windows extracts from misty.exe and
  misty_player.exe).

### Not done yet
- The editor has only its dark theme; much of it uses fixed colours, so the light-visuals capture only checks
  the icon tints. The full new UI style (mockup) is a later task.
- Rect and Transform tools, other lights, prefabs and the remaining components keep their glyphs (no art yet).
- The taskbar icon was not captured live (the user's editor is open); the window icon is set through eframe and
  the exe icon was checked by extracting it.
- The ICO's white flame is faint on light taskbars at 16–24 px (the art's drop shadow is the only outline).

## v0.0.1: branding merged ✅

### Done
- Merged `v001-branding` (decisions renumbered 581–591): official logo and icon set, exe and window icons with
  version info, splash, Help ▸ About, Scene-view tool row, component icons, Project ▸ Search by Type.
- One launcher for the Hub, the splash and the editor; the Hub sidebar uses the official logo; every place shows
  version 0.0.1 from the workspace version.

## v0.0.1: Updater ✅

### Done
- `crates/misty_editor/src/updater/`: GitHub Releases check (background, 6-hour cache in
  `%APPDATA%\MistyEngine\updater.json`, SemVer compare, Skip this version), the "New version X is available — Update now
  / Later / Skip this version" message box with release notes, a green "⬆ Update" pill in the editor toolbar (right),
  Help ▸ Check for Updates…, and the in-place install: download with progress, SHA-256 check against the release's
  `.sha256.txt`, extraction, rename-based self-replace with full rollback, relaunch with the same arguments, cleanup of
  `*.old.exe` on the next start. Read-only installs get "Download the new version" with the release page.
- Repository: `updater::REPO = "Aalzard/MistyEngine"`; publish steps in docs/release.md.
- Tests (no real network): versions, cache/skip, JSON parsing, asset choice, URL allow-list, checksum refusal, extract /
  swap / rollback / zip-slip in temp folders, end-to-end download → verify → install against a local `TcpListener`
  server (with a redirect), a mismatching checksum leaving the install untouched, 404 = no update, offline = silent, and
  kittest UI tests (button + box; Later keeps the button; Skip hides both; Update now → release-page fallback).
- Screenshots (`MISTY_FAKE_UPDATE=0.0.2`): docs/screenshots/v001/updater_message_box.png,
  updater_toolbar_button.png.

### Not done yet
- The Hub hook (sidebar footer button + windows) is documented in docs/release.md for the merge; the Hub isn't on this
  branch.
- Windows only for installing (other platforms get the release-page fallback only when the folder isn't writable; the
  swap itself is platform-neutral but packages are Windows-only).
- No signature check beyond the release's own SHA-256 file (same trust as the GitHub account).

## v0.0.1: updater merged ✅

### Done
- Merged `v001-updater` (decisions renumbered 595–603): GitHub Releases check, green Update pill, verified
  self-replace with rollback, Help ▸ Check for Updates….
- The Hub shows the Update pill and windows too; one updater is passed Hub → editor → Hub (decision 604).

## v0.0.1: license, README and the release package ✅

### Done
- LICENSE (GPL-3.0 text) + RUNTIME-EXCEPTION.md (games built with Misty Engine may use any license); Cargo
  `license-file`; Help ▸ About license line; GitHub-facing README.md.
- `tools/make_release.ps1` makes `MistyEngine-v0.0.1-windows-x64\` (MistyEngine.exe, misty_player.exe,
  Examples\SampleProject from git HEAD, README.txt, CHANGELOG.txt, LICENSE.txt), the zip and its `.sha256.txt`;
  docs/release.md describes it and the smoke test.
- `misty --build <project> <out>`; a packaged editor uses the player shipped beside it; Ctrl+K guides open
  without the source tree.
- Smoke test from the unzipped package (outside the repo, throwaway Hub config): Hub, `--new Standard`,
  SampleProject CloudsDemo, headless build of the Standard project and the built game.
  Screenshots: docs/screenshots/v001/release_hub.png, release_new_standard.png, release_clouds_demo.png,
  release_game.png.

### Not done yet
- Both exes are console-subsystem programs, so a console window opens behind the editor when started by
  double-click.
- Opening a demo scene in a project without `Library/` starts the Scene view at the default camera spot
  (for CloudsDemo that is under the terrain); the Game view is fine.
- The exes are not code-signed (Windows SmartScreen warns; README.txt explains "Run anyway").

## v0.0.1: published on GitHub ✅

### Done
- Source pushed to https://github.com/Aalzard/MistyEngine (public). Commit emails hidden behind the GitHub
  noreply address. The repo's auto-created Apache-2.0 commit was joined with `-s ours`, so GPL-3.0 + the
  runtime exception stays the license.
- Release **v0.0.1** is a full release marked Latest, with three downloads:
  `MistyEngine-Setup-v0.0.1-windows-x64.exe` (the recommended one), the portable zip, and its `.sha256.txt`
  (the updater needs it).
- Windows installer (decision 612): `tools/installer/MistyEngine.iss` (Inno Setup 6), built by
  `make_release.ps1`. It installs per user without an admin prompt and adds a Start menu shortcut, an
  optional desktop shortcut and an uninstaller. Install, launch and uninstall were tested.
- The history was rewritten to remove the "Co-Authored-By" assistant lines; no commit's code changed. The
  old history is kept locally in `backup/with-claude-trailers`.

### Not done yet
- Code signing (SmartScreen still warns) and the console window behind the editor (see above).

## Templates: Standard and PP Ready; Hub thumbnail and Scene view follow the work ✅

### Done
- New Project offers two templates (decision 613). **Standard**: a floating platform and cube, post-processing
  off. **PP Ready**: the same platform on the ocean at a sunset (sun 8° up, in front of the camera), with
  Sparse clouds, light volumetric fog with sharp god rays, screen sun shafts, bloom, ACES and vignette. The
  camera looks from a corner of the platform (40° lens). Empty, Ocean and Landscape were removed.
- On close, the Hub thumbnail becomes the last frame of the view on screen (decision 614).
- The Scene view camera is saved on close and restored on open. A new project starts looking through its Main
  Camera, so PP Ready opens on the film shot.

## UI round: title bar, trees, Inspector cards, Gizmos and Quality ✅

### Done (decisions 616–619, screenshots in docs/screenshots/ui-round/)
- Frameless window with our own title bar (logo and project/scene name centred, flat window buttons) and
  flat Pause / Play / Step.
- Hierarchy and Project trees: stripes, folder-like parents, tree lines and foldable Project folders.
  Hierarchy Folders with an Alt+right-click pastel colour wheel; Move To View / Align With View in the row
  menu.
- Inspector components as rounded cards with bold titles and sub-headers and an aligned label column. No
  black outlines anywhere; soft shades separate areas instead.
- Scene view Gizmos dropdown (icon size, tint, 3D icons) and Quality dropdown (level, FXAA, render scale,
  Bilinear / Nearest / Sharpen). These are shared with Project Settings, the Game view and the player.

### Not done yet
- Window move, resize and maximise not tried by hand on the real window yet.
- MSAA level is still fixed at 4×; asset inspectors (Audio Mixer, Texture import, Preset) are not cards yet.

## Round 11: gameplay ✅

### Done (decisions 623–627; screenshots in docs/screenshots/round11/; detailed notes in docs/pending/r11-*.md)
- **Wheel Collider and VehicleDemo:** raycast suspension, friction curves, motor, brake and steering; Lua
  `WheelCollider`.
- **Skinned meshes:** glTF skins and morph targets, GPU skinning (compute), animation import to `.anim`, the Skinned
  Mesh Renderer with blend-shape sliders, a bone gizmo, and SkinningDemo.
- **Animation rigging:** Rig Builder / Rig, Two Bone IK, Multi-Aim, Chain IK, Damped and Override Transform, and
  RiggingDemo.
- **Timeline:** `.timeline` assets with Activation, Animation, Audio and Signal tracks; the Playable Director; the
  Timeline window (scrub preview, trim, blends, markers); Lua; TimelineDemo.
- **Virtual cameras:** the Camera Brain with priority blends; Virtual Camera with Transposer, Third Person,
  Orbital, Composer, Hard Look At, Collider and Noise; VirtualCameraDemo.
- **UI fixes:** toolbar dropdowns stay open while their controls are used; the Project toolbar is one row; the tab
  drag guide is restyled.

### Not done yet
- Timeline record button and Camera Cut track.
- Live virtual-camera preview in the edit-mode Game view.
- Anti-roll bars.
- Blend-shape keys in Record mode.

## Round 12: content tools ✅

### Done (decisions 628–631; screenshots in docs/screenshots/round12/; detailed notes in docs/pending/r12-*.md)
- **Mesh Builder:**
  - Editable Mesh and New Shape (cube, plane, cylinder, stairs, arch, prism, torus).
  - Object/Vertex/Edge/Face editing with extrude, inset, bevel, subdivide, loop cut, bridge, merge, weld,
    detach, flip and per-face materials.
  - Export to Model; MeshBuilderDemo.
- **Splines:**
  - Spline Container (Bezier modes), Extrude (tube/road), Instantiate and Animate.
  - Scene view knot editing and Draw Spline; Lua; SplinesDemo.
- **Tilemaps:**
  - Grid, Tilemap (chunked), rule and animated tiles.
  - Tile Palette window (paint, box, fill, pick, erase, move), Tilemap Collider 2D.
  - 2D lights with shadow casters; TilemapDemo.
- **Shader Graph:**
  - 50 nodes; WGSL generation through the custom shader system.
  - The Shader Graph window (search, typed ports, blackboard, previews).
  - Dissolve / Hologram / Water Ripple; ShaderGraphDemo.
- **Title bar:** never shows an empty project name.

### Not done yet
- Bevel segments and a UV editor.
- Hex grids and Freeform 2D lights.
- Sub-graphs and copy/paste between graphs.
- Spline knot links.

## Round 13: global illumination, baking and GPU effects ✅

### Done (decisions 632–635; screenshots in docs/screenshots/round13/; detailed notes in docs/pending/r13-*.md)
- **Lightmapper:**
  - Lightmap UVs; Light Mode (Realtime/Mixed/Baked); Contribute/Receive GI.
  - CPU path-traced bake (bounces, sky, emissive, denoise, dilation), with Generate Lighting in the Lighting
    window.
  - Baked Lightmap and UV Charts draw modes; LightmapDemo.
- **Light Probes:**
  - Light Probe Group with Scene view editing and Fill Box.
  - L2 SH bake and Delaunay interpolation for moving objects; Lua; LightProbesDemo.
- **Occlusion culling:**
  - Static Editor Flags, Occlusion Area, and the Occlusion Culling window (Object/Bake/Visualization).
  - Baked PVS and Dynamic Occlusion via Hi-Z; Stats; OcclusionDemo.
- **Visual Effects:**
  - The Effect Graph (spawn/init/update/output blocks, operators) compiled to GPU compute.
  - The Visual Effect component, the Effect Graph window, Lua; VfxDemo with a million particles.

### Not done yet
- Subtractive/Shadowmask modes and a GPU baker.
- Probes on skinned meshes and terrain.
- Portal/adaptive occlusion cells.
- Alpha-sorted particles and mesh output.

## Round 14: systems ✅ and v0.1.0

### Done (decisions 636–641; screenshots in docs/screenshots/round14/)
- **Frame Debugger** (step through a frame draw by draw) and **Memory Profiler** (snapshots, tree map, diff).
- **Test Runner:** Lua EditMode/PlayMode tests, the window, and `--run-tests` with JUnit XML.
- **Save system:** Saveable, slots and thumbnails. **Localization:** String Tables, Localized Text, and the
  language switch.
- **Video Player** (MJPEG AVI, GIF) and **Misty Rich Text** (tags, SDF, effects, links, Arabic/Hebrew RTL).
- **Asset Groups** (addressable bundles, async loading) and **Packages** (manifest, semver, window).
- New Project ▸ **Include sample scenes**.
- README for 0.1.0, and the changelog.

### Not done yet
- Linux/macOS builds: the `crossplatform` branch is paused, as the user asked.
- Round 15 is on hold: the next focus is Cycles-like lighting without ray tracing.

## Graphics: Cycles-like lighting without ray tracing ✅ (first pass)

### Done (decisions 642–643; screenshots in docs/screenshots/gfx/)
- CyclesRoomDemo (realtime and baked) recreating the user's Cycles reference room, with a comparison harness
  that measures patch brightness.
- AgX and PBR Neutral tonemapping, and white balance.
- SSGI with colour bleeding, and Hi-Z SSR.
- Light-shape soft shadows, rectangle-light shadows over the full hemisphere, a temporal shadow denoise,
  contact hardening, and Low–Ultra shadow tiers.
- Stripe and double-counting bugs fixed. The realtime room is now close to the bake.

### Not done yet
- Per-ray probe lookup for off-screen SSGI rays.
- Stronger contact darkening under boxes.
- Brighter floor reflection.
- A foliage lighting test patch: waits for the grass branch.

## Grass, live reload, First Person Player and hitches ✅

### Done (decisions 644–647)
- Grass realism: wind waves, colour and height patches, ground fill, grass shadows with a near-camera map,
  grass in the screen-space prepass. MeadowDemo shows it.
- Foliage lighting test patch (gfx-foliage) with its harness.
- Live reload: scenes and assets reload on change, Ctrl+Shift+R, and the "New build — Restart" pill.
- First Person Player: GameObject ▸ First Person Player, the three-script head bob, FirstPersonDemo and tests.
- Frame-time hitches fixed on MeadowDemo; hitch counter and frame graph in Stats.

### In progress
- Grass look to match the user's reference photo: short uneven turf, colour patches, no visible ground,
  sharp close shadows and contact shadows.
- TAA with motion vectors: no blur or smear, zero jitter when TAA is off, and no cloud shaking.
- MeadowDemo performance (branch meadow-perf, details in pending/meadow-perf.md): GPU time per
  pass (`Renderer::set_pass_timing`, `grass_pass_times`), the grass depth pass / prepass / shadow
  casters skip the flat fans of LOD-shrunk blades, per-blade view culling, the grass sun shadow
  taps share one cascade pick, blades carry their patch / swathe / canopy values, and the Game
  view's VSync toggle (off by default; the 60 Hz VSync capped and halved the frame rate). The
  look is unchanged against meadow-look-1 (diffs within the render-to-render noise).

  | MeadowDemo, RTX 4060 | Low | Medium | High |
  |---|---|---|---|
  | editor Game view (1125×558, Play), before | 158 FPS | 71 FPS | 47 FPS |
  | editor Game view, after | **212 FPS** (4.7 ms) | **98 FPS** (10.2 ms) | **71 FPS** (14.2 ms) |
  | GPU 1920×1080, before | 12.5 ms | 20.2 ms | 27.3 ms |
  | GPU 1920×1080, after | 10.6 ms | 17.6 ms | 23.2 ms |
  | GPU full screen at the level's Render Scale, after | 8.0 ms (1440×810) | 18.2 ms | 23.8 ms |

  Screenshots: `docs/screenshots/gfx/meadowperf_*.png`.

### Not done yet
- Prefab, tree, rock and grass placement brushes for the World Builder.
- Linux/macOS builds are paused. Round 15 is on hold.

## Meadow trees ✅ (first pass)

### Done (decision 648; docs/pending/meadow-trees.md, screenshots `docs/screenshots/gfx/trees_*`)
- Procedural tree assets (`.tree`): oak, linden and birch presets; trunk, limbs aimed at the crown outline,
  side branches, folded leaf-cluster cards; two mesh LODs and a CPU-baked 8-view impostor.
- Terrain trees: prototypes and instances in the terrain data, the Paint Trees tab (brush, Mass Place,
  rendering distances), Assets ▸ Create ▸ Tree, the tree Inspector, trunk colliders.
- GPU-instanced rendering with dithered LOD fades, shadows in the sun's cascades, the screen-space
  prepass, wind shared with the grass gusts and motion vectors for TAA.
- MeadowDemo: five sample trees (Oak, Oak Broad, Young Oak, Linden, Birch) placed in groves, the view ahead
  kept open; the user's First Person Player and head bob values kept (generators reproduce them).
- Original CC0 sample textures from `tools/tree_textures.py` (written with OpenAI Codex, reviewed).

### Not done yet
- See "What's left" in docs/pending/meadow-trees.md.


## Scene camera feel ✅

### Done (docs/pending/scene-camera.md)
- Measured with `MISTY_CAMERA_BENCH` (scripted flight through MeadowDemo: pacing, CPU/GPU, grass
  uploads, input → GPU done latency): the Scene view is CPU-bound while flying, hitches come with
  doubled Scene render CPU time and over-budget single grass chunk uploads; the camera itself
  reacted raw (no easing, cursor-based look stopping at the screen edge).
- Frame-rate independent critically damped smoothing (SmoothDamp-style) for look, orbit, pan,
  zoom and fly velocity; look smoothing defaults to 12 ms (under one 60 FPS frame), fly / wheel
  damping 0.08 s.
- Right mouse + WASD/QE with acceleration, Shift ×4, right mouse + wheel speed (shown briefly in
  the view), raw mouse look with the cursor hidden and confined, wheel zoom towards the cursor,
  eased Frame Selected, Shift + letter shortcuts ignored while flying.
- Camera popover: Field of View, clipping, Camera Speed, Acceleration, Easing, Damping, Look
  Smoothing, Reset; saved per project.

### Not done yet
- Grass first-chunk uploads going over the 2 ms budget (grass renderer).

## Meadow performance, round 2 (branch meadow-perf2)

### Done (docs/pending/meadow-perf2.md, screenshots `docs/screenshots/gfx/perf2_*`)
- GPU grass culling: one compute pass tests every blade the grass passes would draw (view and
  flat-fan tests mirrored from the vertex stage) and writes ordered compact lists with indirect
  draw arguments per chunk and pass (lit, depth, AO prepass, sun cascades, near grass shadow
  map); the vertex stages fetch blades through the lists. Bit-identical to `meadow-trees-1`.
  Also exact: terrain chunks front to back, the AO prepass skips off-screen terrain.
- Renders are deterministic in tests (clouds and fog follow the fixed test clock).
- Quality ▸ Grass Ground Cover (off in every preset): the terrain tints the middle distance's
  ground like the LOD's flat fans did and the lit grass skips the fans.
- Quality ▸ Anti-Aliasing is honoured (2× or 4×; Disabled renders 2× for now); the presets
  keep 4× so the picture is unchanged until the user picks 2×.

  | MeadowDemo GPU ms, 1920×1080, RTX 4060 (wide view) | Low | Medium | High |
  |---|---|---|---|
  | meadow-trees-1 | 11.4 | 18.3 | 23.0 |
  | default (GPU culling) | 10.1 (99 FPS) | 15.8 (63 FPS) | 20.3 (49 FPS) |
  | + Ground Cover | 8.5 | 13.3 | 17.4 |
  | + 2× MSAA | 8.5 | 13.5 | 17.5 |
  | + both | 7.6 (131 FPS) | 11.7 (86 FPS) | 15.5 (65 FPS) |

### Not done yet
- Medium at ≤ 10 ms full screen (11.7 ms with both switches): the grass visibility buffer
  (grass study) is next; 1× MSAA; see "Left" in docs/pending/meadow-perf2.md.


## Misty GI 🧪 (prototype, off by default)

### Done (decision 649; docs/pending/misty-gi.md, screenshots `docs/screenshots/gfx/gi_*`)
- Design: global SDF clipmap from object SDFs + terrain + tree proxies, radiance-cache voxel band,
  irradiance probe clipmap gather (SSGI first, Misty GI for the rest), radiance cascades evaluated,
  reflections, optional hardware RT backend, quality levels, Unity-like UI, roadmap.
- Prototype in `render/misty_gi/` behind Post-process Volume ▸ *Misty Global Illumination* (Intensity,
  Quality, Lightmapped Surfaces), composed into the lit shaders' existing GI texel.
- CyclesRoomDemo: within 1–2 sRGB levels of the path-traced lightmap (white and red/green rooms), nothing
  baked. 1080p RTX 4060: High ≈ 1.1 ms, Medium ≈ 0.6 ms, Low ≈ 0.3 ms; a moving object 0.07 ms.
- Tests: colour bleeding at every quality, off-screen bounce, sky occlusion under a roof, moving light,
  SDF and probe sanity; ignored harnesses for timings and comparison renders.

- Follow-up: grass and trees receive GI, glossy reflections behind SSR, interval merge on the probe levels,
  works with Upscaling; High +1.8 ms (room) / +2.7 ms (meadow) per frame at 1080p.

### Not done yet
- Grass blades and trees don't read the GI term yet (their shaders belong to the grass/tree work); the
  Lighting-window switch, debug views, reflections, radiance-cascade merge: see the roadmap in the doc.

## Round 15 — Editor scripting in Lua (branch r15-editor-scripting)

### Done (docs/pending/r15-editor-scripting.md, docs/scripting.md ▸ Editor scripting)
- Lua editor scripts in any `Editor` folder: their own Lua state with the game API on the edited
  scene, hot reload, never in builds.
- `MenuItem` (menu bar, submenus, validate, shortcuts, Ctrl+K), dockable `ToolWindow`s with a small
  `GUI` API and saved state, `CustomInspector` with `DrawDefaultInspector` and undoable
  `self.target` edits, field attributes (`Header`, `Tooltip`, `Space`, `Range`,
  `HideInInspector`), `OnDrawGizmos(Selected)` and `OnSceneGUI` with `Handles.Position`.
- Editor API: `Selection`, `Undo` (with groups), `EditorPrefs`, `AssetDB`, `EditorScene`,
  `Editor` (dialogs, progress bar, update callbacks), `AssetProcessor` / `OnPostprocessAsset`.
- Errors in the Console with file and line, failing callbacks switched off until their script
  changes, a time budget for runaway loops.
- Sample: Scatter Tool, Align to Floor, Satellite with its Inspector, gizmos and handle.

### Not done yet
- `OnPostprocessAsset` for re-imports of changed files; physics queries in editor scripts.

- Lighting-window switch, debug views, water / custom shaders, octahedral probe radiance: see the roadmap in
  the doc. The meadow's GI is subtle until its ambient / turf occlusion is retuned (art decision).

## Grass visibility buffer (branch grass-vis)

### Done (docs/pending/grass-vis.md, screenshots `docs/screenshots/gfx/grassvis_*`)
- The lit grass pass is replaced by the grass study's B4: a multisampled visibility raster
  (same lists, order, depth test, coverage and alpha to coverage as the lit pass; blade id as
  raw float bits + triangle / facing / coverage), a compute resolve that shades each distinct
  blade fragment of a pixel once with the grass's own lighting (`grass_lit` / `shade_grass`,
  Misty GI hook included; previous wind time for the motion vectors), and a per-sample
  composite at the start of the rest of the main pass (grid, transparents, particles,
  effects, gizmos unchanged). Sample count = Quality ▸ Anti-Aliasing (4× default, 2× where
  the adapter can); internal render size with Upscaling. Lit pass kept as the fallback.
- Look: within 0.07 levels of `meadow-trees-1` at every level and view, no pixel over 8
  levels; shimmer and motion the same; same with Upscaling ▸ Performance and with Misty GI.
- Blade pages: a terrain's grass blades live in a few big storage buffers (the resolve reads
  any blade by id; a page binds once for all its chunks).
- Grass chunk uploads go up in budgeted 8 k-blade pieces: in the Scene camera bench, frames
  with a > 2 ms chunk upload 24 → 4, longest 7.9 → 3.4 ms, hitches 169 → 4.
- Grass uniforms and groups made once per terrain instead of every frame; profiler scopes for
  the render's CPU parts.

  | MeadowDemo GPU ms, 1920×1080, RTX 4060 (wide view) | Low | Medium | High |
  |---|---|---|---|
  | before (main) | 10.6 (94 FPS) | 16.5 (61 FPS) | 21.3 (47 FPS) |
  | **visibility buffer (default 4×)** | **8.0 (124 FPS)** | **11.9 (84 FPS)** | **15.4 (65 FPS)** |
  | visibility buffer, 2× MSAA | 6.3 (160 FPS) | 10.0 (100 FPS) | 13.4 (74 FPS) |

  Editor Game view (docked): Low 224 → 307–315 FPS, Medium 106 → 167, High 78 → 116.

### Not done yet
- Medium at defaults is 11.9 ms full screen (84 FPS); 100 FPS needs 2× MSAA on Medium (a look
  change, the user's call) or more grass work (Grass Ground Cover, a cheaper composite).
