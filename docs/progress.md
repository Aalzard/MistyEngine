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

- Round 16 (ready to become a default): Lighting ▸ Realtime Global Illumination (per scene) and
  Quality ▸ Global Illumination (Off / Low / Medium / High per level); water, transparent and
  custom-shader / Shader Graph surfaces receive GI; the editor's Scene view shares the Game view's
  clipmaps when near; Misty GI's sun checked against virtual shadow maps; meadow ambient option
  shots (`gi_meadow_opt_*_compare.png`) for the user to pick from.

- MeadowDemo ships with Misty GI: the user's option C (Realtime Global Illumination on, Environment
  Intensity 0.6) in the scene and its generator; golden shots updated; +0.7 / 1.4 / 2.4 / 2.3 ms at
  Low / Medium / High / Ultra (1080p); before / after in `gi_meadow_shipped_compare.png`.

### Not done yet
- New scenes still start with Realtime Global Illumination off (MeadowDemo has it on).
- Debug views (SDF / radiance cache / probes), directional far field, screen-space near field,
  toroidal clipmap scrolling, skinned meshes: see the roadmap in the doc.

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

## Round 15 — Multiplayer networking (branch r15-networking)

### Done (docs/pending/r15-networking.md, docs/scripting.md ▸ Networking)
- UDP transport on its own thread: reliable ordered / reliable unordered / unreliable channels, handshake,
  keep-alives, timeouts, fragmentation, a latency / jitter / loss simulator; LAN discovery by broadcast.
- Network Manager (host / server / client, address and port, tick rate, player prefab, connection
  approval, scene sync on connect and on scene changes), Network Object (spawn, owner, despawn, ownership
  changes), Network Transform (server or owner authority, interpolation buffer, snapping, thresholds,
  client-side prediction with reconciliation).
- Network Variables (`NetworkVariable()` attribute: read / write permissions, change callbacks, only
  changed fields sent), `…ServerRpc` / `…ClientRpc` with `RpcTarget`, the `Network` Lua API with events.
- Editor: Netcode Inspectors (Start Host / Server / Client while playing), Window ▸ Multiplayer ▸
  Multiplayer Play Mode launching up to three player windows as clients, a network stats overlay.
- Player: `--net-host`, `--net-server`, `--net-client`, `--scene`, window options, stats in the title.
- Sample: NetworkDemo (predicted capsules with names, colours and a replicated score, a server-run coin).

### Not done yet
- Relay / NAT punch-through, encryption, dedicated-server (headless) builds, Network Animator, interest
  management, tighter bandwidth (quantisation, batching).

## Virtual shadow maps (branch vsm)

### Done (docs/pending/vsm.md, screenshots `docs/screenshots/gfx/vsm_*`)
- Quality ▸ Shadows ▸ Cascaded / Virtual (every preset Cascaded), Shadow Page Pool (512–4096 pages),
  Shadow Resolution = texels per pixel in Virtual mode.
- Sun clipmap of 8k × 8k levels around the camera (1 mm texels close up at High), 128² pages on a
  light-space grid, toroidal page table, a coarse safety level over the whole shadow distance.
- Page marking from the depth buffer (grass included) after the main pass, asynchronous readback, LRU page
  cache; only new pages and pages under moving / new / removed / posed casters, changed terrain chunks or a
  turned sun are rendered — 0 pages per frame on a still camera, ~1 while walking in MeadowDemo.
- Pages rendered per rectangle of neighbouring pages through the existing caster pipelines (meshes, terrain
  chunks, trees with per-level detail, virtual geometry) into a scratch atlas, copied into the pool.
- Sampling in every lit shader (meshes, terrain, grass lit and visibility resolve, trees, water, custom
  shaders, the temporal shadow mask) with PCSS search / filter at kernel-sized levels across page borders;
  fog and sun shafts read cascade layers filled from the virtual map.
- Scene view draw modes Virtual Shadow Levels / Cache / Pages; `Renderer::vsm_stats`.
- MeadowDemo High: 15.44 → 15.17 ms still, 15.45 → 15.24 moving (wide); Medium +0.15–0.4, Low +0.2.

### Round 2 (branch vsm2, default-ready)
- Near layer redrawn every frame for casters that sway: grass past the near grass map (High 20 m, Ultra
  32 m) and trees near the camera (left out of the cached pages; pages re-render as they enter or leave).
- Turning sun: a second page set fills in the background and takes over (no full re-render, no dropout).
- Guard band: neighbours of marked pages requested too (late pages on a 90°/s turn 8.3 → 0.1 a frame at
  Ultra); marking from the screen-space prepass's depth or every other frame; one table load per pixel.
- Ultra's 4096-page pool fixed (was over the 8192-texel limit).
- MeadowDemo wide, GPU ms cascaded → virtual: Ultra 21.33 → 19.17, High 15.25 → 15.15, Medium 11.96 →
  12.13, Low 8.08 → 8.29 (still); faster or equal at Ultra / High walking, turning and with a moving sun.
- Recommendation (defaults unchanged): Ultra and High could switch to Virtual; Medium and Low stay Cascaded.

### Not done yet
- Medium / Low remain 0.15–0.45 ms slower in MeadowDemo; trees too large for the near layer keep a cached
  wind pose; a sun jump over 5° re-renders everything.

## Freeze on entering Play / opening a scene (branch play-hitch)

### Done (docs/pending/play-hitch.md)
- The "worst 3.4–4.4 s" in the Stats box was the load frame of a capture run (project open,
  scene load, entering Play and the scene's first render in one frame), 3.5–5.7 s on a new
  build of the editor (NVIDIA's shader cache is per executable), plus 2–3 frames of 200–600 ms
  when the tree textures arrived.
- Terrain layer arrays (370–440 ms) and tree textures (85–105 ms per tree) are built on worker
  threads and uploaded in budgeted pieces; the terrain draws neutral layers for a few frames.
- Tree models cached in `Library/TreeCache` (keyed by recipe, textures and the generator's
  source); a wgpu pipeline cache in `Library/ShaderCache` (Vulkan); shader modules made once
  per source; the tree, grass visibility, grass cull, volumetrics and cloud pipelines, the cloud
  noise, grass atlas and blue noise prepared on threads as soon as the renderer exists.
- Asset groups, string tables and editor scripts list files from the GUID database instead of
  reading every meta file / walking `Assets/` on the main thread (two 50 ms reads and a 25 ms
  walk when entering Play; a 50 ms walk every 2 s from the editor scripts).
- Capture runs: `MISTY_PROFILE`, a slow-frame / time-to-interactive report, `MISTY_PLAY_AT`,
  `MISTY_OPEN_SCENE_AT`.

  | MeadowDemo, RTX 4060 | before | after |
  |---|---|---|
  | Entering Play in a running editor (worst frame) | 140–192 ms | 13–28 ms |
  | Opening MeadowDemo in a running editor, warm / new build | 613–631 / 2476–2514 ms | 37–64 / 35–60 ms |
  | Capture run (open + Play at launch), new build: load frame / worst after / interactive | 4.9–5.0 s / 616–625 ms / 6.7–6.9 s | 0.5–0.8 s / 30–37 ms / 1.5–2.7 s |
  | Same, warm | 1.2–1.3 s / 187–296 ms / 2.8–3.2 s | 0.33–0.38 s / 46 ms / 1.0–1.2 s |
  | Same, first launch ever (no Library, new build) | 5.0–5.7 s / 600–980 ms / 6.7–8.4 s | 3.2–3.7 s / 40–50 ms / 4.2–5.9 s |

### Not done yet
- Project open (`EditorState::new`, the GUID scan, the core pipelines) is most of what is left
  of the load frame (~0.33 s warm); the very first launch still compiles the core pipelines on
  the main thread (behind the splash).

## Test health (branch test-health)

### Done (docs/pending/test-health.md, references `docs/golden/`)
- `cargo test --workspace --release` green twice in a row (editor 582, engine 951, player 11
  passed; 0 failed).
- `foliage_lighting_demo_matches_its_generator`: the generator used `MAX_DENSITY`, raised from
  600 to 1500 for MeadowDemo after FoliageLightingDemo was committed; it now says 600, the
  committed value.
- Hitch test: slow frames are judged by the render thread's own CPU time and the machine's
  load, so a loaded machine's stalls are printed, not failed; the profiler tests and timing
  tests share a lock; `headless()` switches grass streaming off only under the GPU lock.
- Asset watcher test: own database and walk (not the process-wide ones); `poll_now` no longer
  reads a walk that started before the change; waits with a timeout.
- GPU tests: one process-wide wgpu instance (no Vulkan driver unload/reload after every test),
  prebuilt workers of an earlier test finish before the next device. The round-14 `0xc0000005`
  did not reproduce in 12 engine runs (default and one thread, two processes at once under load).
- Golden shots: `cargo test --release -p misty_engine --lib golden_shots -- --ignored --nocapture --test-threads=1`
  compares MeadowDemo wide / close / behind / oak and CyclesRoomDemo at every quality level with
  the references; `MISTY_GOLDEN=update` rewrites them. The gate for "flip a default" changes.

### Not done yet
- The access violation was not reproduced, so the instance change is a likely fix, not a
  proven one; watch for it in future full runs.
- The golden references are for the RTX 4060 / NVIDIA 610.88 / Vulkan; another GPU or driver
  needs its own (`MISTY_GOLDEN=update` on it) or may drift past the tolerance.

## Linux x64 release build (branch linux-ci)

### Done
- The workspace type-checks for `x86_64-unknown-linux-gnu` from Windows (`cargo clippy --workspace
  --all-targets --target x86_64-unknown-linux-gnu`, 0 warnings). The C parts (ring, mlua's
  vendored Lua) and the pkg-config probes (alsa-sys, libudev-sys) can't build on a Windows host;
  the check used do-nothing `CC_*` / `AR_*` / `PKG_CONFIG_*` shims, so only the Rust code was
  checked there. Real Linux builds come from GitHub Actions.
- Windows-only code was already gated (`AttachConsole`, `AllocConsole`, the locale call, memory
  counters, the hitch test's `QueryThreadCycleTime` / `GetSystemTimes` with Linux
  `CLOCK_THREAD_CPUTIME_ID` / `/proc/stat` variants, exe icon embedding, `CREATE_NO_WINDOW` for
  cargo, `cmd /C start` / `explorer` with `xdg-open` fallbacks; executables are found with
  `EXE_SUFFIX`, so Multiplayer Play Mode finds `misty_player` on Linux). The only Linux warning,
  the unused `icon::group_resource`, is now `cfg(any(windows, test))`.
- Updater: `release::Platform::HOST` picks the package per OS (`-windows-x64.zip` on Windows,
  `-linux-x64.tar.gz` on Linux x64; never the Setup `.exe`; other systems get the release-page
  fallback). The `.tar.gz` is unpacked in-process with the `tar` crate (regular files and folders
  only, `..` / absolute paths and links refused), file modes kept; the swap moves the running
  extension-less binary to the backup folder (allowed on Linux). `PACKAGE_EXE` is `MistyEngine`
  on Linux. New tests: Linux asset choice, tarball extract + swap (executable bits on Unix),
  tar slip / symlink refusal, end-to-end tarball install against the local test server.
- `tools/make_release_linux.sh`: the Linux package, same layout as Windows (`MistyEngine`,
  `misty_player`, `Examples/SampleProject` from `git archive HEAD`, README.txt from
  `tools/release/README-linux.txt`, CHANGELOG.txt, LICENSE.txt) as
  `MistyEngine-vX.Y.Z-linux-x64.tar.gz` (GNU tar, root-owned, sorted) + `.sha256.txt`.
- `.github/workflows/release.yml`: on `v*` tags (or by hand with a tag), ubuntu-22.04 (glibc
  2.35): checks the tag equals the Cargo.toml version, installs the -dev packages, builds
  `misty_editor` + `misty_player` in release (cargo cache), packages, checks `ldd` and the CLI
  usage exit code, uploads an artifact, waits up to 45 min for the tag's release (a draft
  counts) and uploads the two files with `gh release upload --clobber`; creates a published
  release from the CHANGELOG section if none appears.
- `.github/workflows/ci.yml`: by hand or on `ci/*` pushes, `cargo test --workspace --release` on
  ubuntu-22.04 with no Vulkan driver (GPU tests skip).
- README-linux.txt: extract and `./MistyEngine`, runtime libraries for apt / dnf / pacman,
  Vulkan drivers, Wayland vs X11 (`env -u WAYLAND_DISPLAY`), `WGPU_BACKEND=gl`, file dialogs
  through xdg-desktop-portal or zenity. docs/release.md describes the Linux package and workflow.

- Linux test run (branch linux-tests): the first `ci/linux` run built fine; 10 sample
  `*_matches_its_generator` tests failed because they compared regenerated files to the
  Windows-made ones byte for byte, and Linux's libm `sin` / `cos` / `pow` differ in the last bit.
  New `misty_editor::test_util` (test-only): `assert_json_close` (text around the numbers
  identical, integers equal, floats within 1e-5 relative / 1e-6 absolute), `assert_value_close`,
  `assert_close` (any `Serialize` value: materials, clips, particle systems…), `assert_glb_close`
  (JSON chunk as above, BIN chunk exact except FLOAT accessor elements) and `assert_image_close`
  (each channel within one step, at most 1% of them). All 50 generator tests use it; its unit
  tests prove real changes (a moved float, an integer, a string, a key, an index) still fail.
  Failure messages list the first differences instead of dumping whole files (the old glb
  `assert_eq!` dump cut off the CI log).
- World Builder erosion amplified the libm noise into different heights, so World Builder now
  uses `world_gen::portable` (sin / cos / tan / atan / atan2 / pow from the pure-Rust `libm`
  crate): the same seed builds the same terrain on every OS. WorldBuilderDemo regenerated.
- CI: `--no-fail-fast` output goes to `test-results.txt` (uploaded as the `test-results`
  artifact) and the run summary lists every crate's `test result` line and failures.

### Not done yet
- No `.desktop` file or icon install, no AppImage / Flatpak; the window icon is set at runtime
  (X11), Wayland shows the generic one.
- Linux ARM64 has no release package (the updater sends those users to the release page).

## Tree look round 2 (branch tree-look)

### Done (docs/pending/meadow-trees.md, "Look round 2")
- Bark no longer renders inside-out: the trunk and branch tubes were wound against the engine's
  clockwise front face, so back-face culling drew the inner walls; leaf cards' front faces now
  face out of the crown too. Test `bark_faces_point_out_of_the_limbs`.
- Codex-written PBR texture generator (`tools/tree_textures.py`, 5 review rounds): 1024²
  tileable bark per species (albedo, normal, roughness, AO, height) and new leaf atlases
  (albedo, normal, roughness, translucency). New optional recipe slots; packed into spare
  channels (one texture pair per draw); bark roughness, occlusion and offset parallax, leaf
  roughness and translucency, softer light through the leaves.
- Thicker, gnarled oak limbs, root buttresses, surface bark normals, bark texture scaled to the
  trunk, smaller and denser cards towards the crown outline. Tree cost unchanged within noise
  (0.0–1.3 ms on High).

### Not done yet
- Bark close up is much better but still procedural (furrow pattern somewhat regular on the
  linden, oak a little dark in shade); leaf cards seen edge-on still show stretched texture.

## MeadowDemo project template (branch meadow-template)

### Done
- New Project has a third template, **MeadowDemo**: "A walkable meadow with a first-person player:
  thin grass, trees, Misty GI. Free assets for your game." It copies `SampleProject`'s MeadowDemo
  scene and everything it uses, found with the build's dependency walk (`asset_deps`, which now
  also follows `.tree` recipes to their textures, so built games get the tree textures too): the
  terrain, the 5 tree recipes with their textures, README and LICENSE, the meadow and terrain
  ground textures, the First Person Player prefab and its 4 scripts, every `.meta` file (same
  GUIDs, folders included) and the sample's project settings (Run / Crouch axes, the Player
  layer). From `Examples/SampleProject` next to the exe or, in a dev build, the committed
  `SampleProject` (`git archive HEAD`). The project opens on `MeadowDemo.scene`.
- The new project looks towards the sun: the Player is turned to the sun's horizontal direction
  (nudged if a tree stands right in front) and the Scene view starts just behind its eyes,
  slightly down. Only the copy is turned; the sample scene is unchanged
  (`docs/screenshots/gfx/meadow_template_start.png`, `meadow_template_player.png`).
- Every template card has a one-line description (`docs/screenshots/gfx/hub_templates.png`);
  the MeadowDemo card is dimmed when the install has no sample project. "Include sample scenes"
  works on top of it (files already there are kept). Tests: copy list and GUIDs, every reference
  of the scene, prefab, terrain and trees resolving, opening and 60 frames of Play without an
  error, the folder (release) source, the Hub card and creating it from the Hub.

## Meadow rocks (branch rocks)

### Done (docs/pending/rocks.md)
- Six procedural rock types in `SampleProject/Assets/Rocks` (CC0, README + LICENSE): Boulder
  Large, Boulder Split, Boulder Round (granite), Outcrop Layered (sandstone slab), Rock Small and
  Cobbles. Each: a dense LOD 0 (82 k triangles for the big ones) imported with Virtual Geometry
  on, three simplified LOD levels, its own baked albedo / normal / mask maps (lichen, moss on
  top faces and in hollows, a grass-tinted damp base), a material and a LOD Group prefab with a
  Mesh Collider. Generator `crates/misty_editor/src/sample_rocks` (`write_rocks`); tileable
  source sets from the Codex-written `tools/rock_textures.py`.
- MeadowDemo: 64 rocks (34 big, 30 small) in 21 groups (boulders and outcrops near the rises and along the foot of
  the mountains, a few in the open field's sides, small rocks and cobbles round the big ones),
  sunk and tilted to the ground, clear of trees, the player and the view ahead. Only the
  scene's "Rocks" object is generated (`write_meadow_rocks`); the user's objects stay.
- LOD Groups whose LOD 0 is a virtual geometry mesh keep LOD 0 at every distance while virtual
  geometry draws it (`LodRange::virtual_lod`), and switch levels as usual otherwise.
- Shots `docs/screenshots/gfx/rocks_*`, GPU cost test `rock_cost`: LOD Groups cost 0–0.7 ms;
  virtual geometry measured ~1–1.4 ms more (Low 2–4 ms), so the Quality presets keep it off.
  Meadow golden references updated; CyclesRoom unchanged.

### Not done yet
- No prefab brush / terrain paint for rocks (optional part); moss follows the object's up axis
  (baked); boulders still a little loaf-like at a distance; GI bounces the material colour.

## Flowers and leaves (branch flowers-leaves)

### Done (docs/pending/flowers.md; docs/pending/meadow-trees.md, "Look round 3")
- Wildflowers: up to four kinds per terrain (Terrain Inspector ▸ Paint Texture ▸ Flowers),
  growing in low-frequency patches per kind with clumps, a few lone flowers, more around trees
  and where the grass thins, fewer on worn spots, plus a Paint Flowers mask. Drawn as grass
  blades of their own kind (two crossed plant cards and a head card each), so they stream,
  cull, go through the grass visibility buffer, cast shadows and sway with the grass's wind
  with correct motion vectors. Four CC0 sample wildflowers in `SampleProject/Assets/Flowers`
  (purple knapweed, red poppy, white oxeye daisy, yellow buttercup) from the Codex-written
  `tools/flower_textures.py`; MeadowDemo grows them in patches. Cost 0.0–0.16 ms.
- Leaves: new Codex-drawn leaf atlases (real oak lobes, heart-shaped linden, deltoid birch
  leaves, veins, per-leaf colour, translucent blades with darker veins), much lower roughness
  (a waxy sheen and glints), the leaves' own normals count more, LOD 0 leaf cards curve back
  at the top, more light through backlit leaves. Tree cost within 0.05 ms of before.
- Meadow golden references updated (after merging the rocks); CyclesRoom unchanged.

### Not done yet
- Flower plant cards are flat; seen from straight above only the head card shows. No flower
  colour in the far terrain beyond the grass draw distance (160 m in MeadowDemo).
- Leaf cards seen edge-on still show stretched texture; linden and birch leaves are still a
  little schematic close up.

## Shadow stutter (branch shadow-stutter)

### Done (docs/pending/shadow-stutter.md)
- Repro harness `render/shadow_stutter_tests.rs`: a scripted First Person walk / turn through
  MeadowDemo that reads every sun cascade and near grass shadow map slice back each frame and
  counts the world texels whose caster changed (the maps are world-snapped, so any change is
  a caster that popped because the camera moved), plus an on-screen metric (reprojected
  frame-to-frame change in shadow, penumbra and lit regions).
- Causes found and fixed: terrain chunks drawn into the shadow maps at the camera's LOD (a
  whole 256 m chunk popped in cascade 3 with long shadow distances; a chunk flipped with the
  stride bob), trees switching their cascade-0 shadow LOD with the camera's distance, and the
  near grass shadow map shrinking its blades inside the part the lit shaders read (12.3 % of
  the inner slice changed every walking frame, 1.5 % now) plus grass shadow cards re-cut per
  blade.
- Shadow maps draw terrain at full detail (camera LOD only for coarse receivers), culled per
  map; tree shadows use a fixed LOD per map (by its texel size, as the virtual shadow map);
  near-map casters fade out past its read area. Cost +0.15 ms on High, ±0 on Medium and Low;
  4 MeadowDemo golden references updated (grass shadow detail close up, Ultra tree crowns).

### Not done yet
- Cascade grass casters still fade smoothly with the camera's distance (near-map hand-over,
  grass LOD); the camera's terrain LOD has no hysteresis (main view only now).

## Post effects and decals (branch postfx, round 17)

### Done (docs/pending/postfx.md)
- Post-process Volume overrides, all off by default: **Depth of Field** (Gaussian; Bokeh with
  focus distance, focal length, f-stop, blade count / curvature / rotation and Low / Medium /
  High quality, half-resolution near and far fields), **Motion Blur** (per object and camera
  from the motion vectors, intensity, maximum velocity, sample count, camera clamp) and **Auto
  Exposure** (compute histogram, Average / Spot / Center Weighted metering, EV limits,
  compensation, Fixed / Progressive adaptation). They run after TAA / the upscaler at the output
  resolution; Inspector sections in the volume. The Scene view leaves motion blur out.
- **Decal Projector** (GameObject ▸ Rendering ▸ Decal Projector): box-projected base colour with
  opacity, normal, mask (metallic, occlusion, smoothness) and emission, angle and distance fades,
  priority. Screen-space decals from the screen-space prepass's depth and normals, blended into
  a per-pixel decal buffer that the lit shaders apply before lighting (URP's DBuffer idea in a
  forward renderer). Meshes, virtual geometry and terrain. Inspector, gizmo, icons.
- Procedural CC0 decal textures (dirt, moss, cracks, puddle) and the PostFxDemo sample scene.
- Costs at 1080p: Gaussian DOF 0.27 ms, Bokeh 0.47 / 0.64 / 0.87 ms, motion blur 0.62 ms at 8
  samples, auto exposure 0.03 ms, 200 decals 0.33 ms including the prepass they turn on.
  Shots `docs/screenshots/gfx/postfx_*`. Golden shots unchanged.

### Not done yet
- Physical camera properties on the Camera for Bokeh focus; decal layers; decals on transparent
  surfaces; local volumes, film grain, chromatic aberration and lens distortion.

## One-click virtual geometry (branch vgeo-button)

### Done (docs/pending/virtual-geometry.md, "Round 3 of vgeo-button")
- **GameObject ▸ Enable / Disable Virtual Geometry** (also in the Hierarchy context menu, only
  the one that applies, and as a Mesh Filter Inspector button): turns the Virtual Geometry import
  setting on or off for the models of the selected objects and their children, re-imports them
  and shows the cluster builds in a small progress window. Skipped meshes go to the Console with
  a reason.
- **Tools ▸ Rendering ▸ Convert Scene / Project to Virtual Geometry** (Tools is a built-in menu
  now; editor-script items join it): a short dialog with the triangle threshold (default 5 000),
  "N models, M objects, est. memory", a Quality ▸ Virtual Geometry checkbox for High and Ultra
  when they have it off, and a collapsed list of what is skipped and why (skinned, blend shapes,
  alpha-tested, transparent, double-sided, unlit or custom-shader materials, under the
  threshold, LOD 1+ of a LOD Group). Each conversion is one undo step that puts the import
  settings and the Quality levels back. Ctrl+K lists all four commands.
- **Lower fixed cost**: virtual geometry now picks its paths per frame (Auto): clusters are drawn
  lit unless their triangles are small enough for the visibility buffer to pay, and the main
  camera is culled in one phase (no Hi-Z pyramid, no split main pass) below 16 384 instances or
  without GPU culling; all shadow maps are culled by one dispatch per pass. MeadowDemo's rocks
  through virtual geometry now cost the same as or less than their LOD Groups on Medium–Ultra
  (they were 1.0–1.6 ms slower).

### Not done yet
- No LOD Group fallback mode (not needed after the fixed cost went down); no persistent GPU
  instance buffer (instances are still re-uploaded every frame); per-tile material
  classification of the visibility resolve.

## Prefabs 2.0 (branch r18-prefabs)

### Done (docs/pending/r18-prefabs.md)
- Per-property overrides on stable object ids (`Prefab Source`), with added / removed components
  and child objects; instances follow asset changes in place and keep their overrides (Apply, Apply
  All on other instances, Prefab Mode saves, live reload of `.prefab` files, scene reloads through
  the override record saved in scene files).
- Inspector: Overrides (n) ▾ dropdown with per-item Revert / Apply and Revert All / Apply All; bold
  overridden labels with a blue bar, right-click ▸ Apply to Prefab (outer or nested) / Revert;
  per-component Apply / Revert in the ⋮ menu; removed components listed; red Missing Prefab state.
- Prefab Mode: open from the Project, the Hierarchy's › arrow or the Inspector; breadcrumb bar
  (Scenes ▸ Cart ▸ Wheel), Save / Ctrl+S, Auto Save, In Context (scene dimmed behind), nested
  levels, Save / Discard / Cancel in the bar.
- Nested prefabs propagate (outer prefab files rewritten); Prefab Variants (Create ▸ Prefab Variant)
  inherit through chains; Unpack / Unpack Completely; model files as read-only prefabs.
- Format additions are backward compatible; SampleProject and the generator tests unchanged.

### Not done yet
- Closed scenes keep old instance copies until opened (builds pack them as saved).
- A few component editors label fields differently from the field name: those overrides show on the
  header bar and in the dropdown but not as a bold label.
- No comparison view in the Overrides dropdown, no per-element list overrides.

## Round 18C — Input System (branch r18-input)

### Done (docs/pending/r18-input.md)
- Action-based input next to the Input Manager (`input::actions`): Input Actions assets (`.inputactions`) with
  action maps, Value / Button / Pass Through actions, binding paths for keyboard, mouse, gamepad, touch and
  pointer, composites (2D Vector, 1D Axis, one / two modifiers), processors (invert, normalize, scale,
  clamp, axis and stick dead zones), interactions (press, hold, tap, slow tap, multi-tap) and control schemes.
- Player Input component (Send Messages `OnMove(value)` / `OnJump()`, Broadcast Messages, Invoke Events with
  per-action calls), control scheme auto-switching, device pairing for local multiplayer, device lost /
  regained.
- Lua: `InputActions.Load`, `ReadValue`, `IsPressed`, `WasPressedThisFrame`, `WasReleasedThisFrame`, `started`
  / `performed` / `canceled` listeners, `Enable` / `Disable` maps and actions, binding overrides, interactive
  rebinding saved to PlayerPrefs, `Gamepad.current:SetMotorSpeeds`, touches, `Keyboard.current` /
  `Mouse.current`. Active Input Handling (Input Manager / Input System / Both); the Input Manager is unchanged.
- Editor: Input Actions window (maps | actions and bindings | properties, path picker, Listen, schemes,
  interactions and processors, Save Asset / Auto-Save), Player Input Inspector, Assets ▸ Create ▸ Input
  Actions. Player: touch events and rumble.
- Sample `InputSystemDemo` (First Person Player on `Player.inputactions`, rumble on hold, a rebinding menu);
  `FirstPersonController.lua` uses a Player Input when its object has one. Screenshots
  `docs/screenshots/r18/input_*`.

### Not done yet
- No PlayerInputManager (joining / split screen), no UI input module (the UI map doesn't drive the UI), no
  on-screen controls, no undo inside the Input Actions window; rumble untested on real pads.

## Animator depth (branch r18-animator)

### Done (docs/pending/r18-animator.md)
- Layers: Override and Additive blending with weights (Lua `SetLayerWeight`), Avatar Masks (`.mask`
  assets with a transform tree editor and skeleton import), synced layers with their own clips.
- Blend trees: 1D, 2D Simple Directional, 2D Freeform Directional, 2D Freeform Cartesian and Direct,
  per-child speed; a diagram in the Inspector and a blend tree view in the Animator window with a
  draggable parameter marker and the selected motion's influence.
- Sub-state machines with their own Entry / Exit / default state, Entry transitions, transitions from
  a machine node on exit; graph breadcrumb, (Up) node, Move to; full state paths in Lua.
- Transitions: Interruption Source (five modes) with Ordered Interruption and a frozen interrupted
  pose, Any State interrupting any blend, Can Transition To Self, Mute / Solo; state Multiplier and
  Tag.
- Root motion: Apply Root Motion (object or bone root, through a Character Controller),
  `OnAnimatorMove`, `deltaPosition` / `deltaRotation` / `velocity`, `ApplyBuiltinRootMotion`;
  `MatchTarget`; damped `SetFloat`; `CrossFade` / `Play` on any layer.
- Humanoid: `.avatar` assets auto-mapped by bone names with the rest pose, clips converted to
  humanoid clips and retargeted onto other rigs (other names, axes, proportions).
- CharacterAnimatorDemo sample (2D locomotion with root motion, masked wave layer, additive breathing,
  Emotes sub-machine, a second rig playing the retargeted wave). Old controllers and scenes load
  unchanged.

### Not done yet
- Automatic foot IK / IK pass (Two Bone IK constraints work), muscle-space retargeting and editable
  avatar mappings, nested blend trees, state machine behaviours, root motion on Rigidbodies.

## MeadowDemo performance, round 3 (branch perf3)

### Done (docs/pending/perf3.md)
- Same picture, less work (each behind a switch in `render/fast_paths.rs`, all on;
  `MISTY_FAST_OFF` turns them off in any build): the terrain drawn after the trees and the
  grass's depth, the sky drawn last on the far plane, the grass visibility raster's standing
  blades writing the grass depth themselves (the blades are rasterised once, the terrain is
  shaded against their depth), the lit cull lane holding only the flat fans.
- Misty GI updates its probes only where lighting changed: levels whose shapes, placement,
  lights and sky have stayed the same for 128 updates trace their converged probes one turn
  in eight (probe trace 1.06 → 0.17 ms on High); any change brings the full rate back.
- Terrain layer heights in the normal array's alpha (one fetch and a third of the layer
  memory less), zero-weight turf work skipped; one bilateral upsample for Misty GI's
  irradiance and reflections; tree bind groups kept across frames.
- MeadowDemo at 1080p, steady state, wide / behind view: High 17.97 → 14.97 ms (56 → 67 FPS) /
  20.09 → 16.36 ms, Medium 13.25 → 11.19 ms (75 → 89 FPS) / 14.52 → 11.64 ms, Low 9.08 →
  7.57 ms (110 → 132 FPS), Ultra 24.05 → 21.06 ms; editor Game view High 97 → 113 FPS,
  Medium 148 → 166 FPS. Golden shots unchanged (meadow mean ≤ 0.01).
- Proposed (not applied) quality defaults with measured gains and look diffs: Upscaling
  Quality on Medium / High, Grass Ground Cover, Virtual shadow maps on Ultra; Virtual
  Geometry for the rocks costs the same as LOD Groups in these views.

### Not done yet
- Cheaper sharp sun shafts; the walking cost of Misty GI (toroidal clipmap scrolling would
  keep settled probes); the terrain's shadow lookups where the shadow mask can't serve them.

## Round 18D — Asset pipeline (branch r18-assets)

### Done (docs/pending/r18-assets.md)
- FBX import through ufbx (binary and ASCII): meshes per material part, hierarchy, skins and skeleton, blend
  shapes, baked animation stacks, PBR-mapped materials with resolved or embedded textures. FBX models are
  read-only prefabs with Skinned Mesh Renderers and an Animation, like `.glb` ones.
- Model import settings in the Inspector (Revert / Apply): Scale Factor, Convert Units, Axis Conversion,
  Normals (Import / Calculate with Smoothing Angle / None), Tangents, Read/Write, Generate Lightmap UVs,
  Virtual Geometry, Generate LODs (vgeo simplifier, LOD Group on instances), Extract Materials… to a folder,
  clip splitting by frame range with a clip preview.
- GPU texture compression: Compression None / Normal Quality (BC1, BC3) / High Quality (BC7), BC5 normal maps,
  BC6H encoder for HDR; platform overrides; cached in `Library/TextureCache` by content hash and settings,
  encoded in the background in the editor, packed in builds, uncompressed without `TEXTURE_COMPRESSION_BC`.
  MeadowDemo's material textures go from 125.8 MB to 21.0 MB; goldens unchanged (mean ≤ 0.01).
- Samples `Tentacle.fbx` (Z-up, centimetres, rigged, Bend take, blend shapes) and `Crate.fbx`, made in code.

### Not done yet
- Tree, grass/flower and terrain-layer textures still upload RGBA8 (their own loaders; would save ~167 MB more
  in MeadowDemo). No ETC2/ASTC fallback, no BC6H skies, no Humanoid rigs, LODs not cached in Library.

## Round 19A — uGUI parity (branch r19-ui)

### Done (docs/pending/r19-ui.md)
- Canvas render modes: Screen Space - Overlay, Screen Space - Camera (Render Camera, Plane Distance; unlit,
  sorted with the scene by depth) and World Space (placed by the Transform, sized by the Rect Transform,
  drawn with depth in the UI pass and clicked through the camera). Canvas Scaler: Constant Pixel Size, Scale
  With Screen Size (Match Width Or Height, Expand, Shrink), Constant Physical Size, Dynamic Pixels Per Unit.
- Image types: Simple, Sliced (9-slice borders from the sprite import settings, edited there with preview
  lines), Tiled, Filled (horizontal, vertical, radial 90 / 180 / 360 with origin, amount, direction),
  Preserve Aspect. Canvas Group, Aspect Ratio Fitter. Rect Transforms turn and scale with the Transform.
- Navigation: Selectable Navigation (None, Horizontal, Vertical, Automatic, Explicit, wrap around, Visualize),
  Event System First Selected, Submit, Cancel and move repeat, driven by the Input System's UI map (the Event
  System's Actions Asset or the default UI actions) with the Input Manager as fallback.
- Scene view: every canvas drawn (2D and 3D), outlines, click to select UI, Rect tool handles (resize, move,
  rotate, pivot, anchors).
- Font assets: SDF or smooth raster atlases, sampling size, padding, fallback fonts, with an Inspector preview;
  used by Text and rich text.
- Lua: Image fill / type, Canvas, CanvasGroup, AspectRatioFitter. Sample `UIShowcase` (gamepad menu, radial
  health ring, sliced panels, World Space sign, Screen Space - Camera HUD). Screenshots
  `docs/screenshots/r19/ui_*`.

### Not done yet
- No scripted selection (`SetSelectedGameObject`), no Blocking Objects for world canvases, rotated sliders map
  the pointer unrotated, no draggable border handles in the import preview, no baked-charset font assets.

## Ragdolls and cloth, round 19C (branch r19-physics)

### Done (docs/pending/r19-physics.md)
- Configurable Joint (per-axis Locked / Limited / Free, hard or spring limits, linear and angular
  drives with targets and Maximum Force, projection) and Character Joint (twist and two swing
  limits); Break Force / Break Torque on every joint with `OnJointBreak(breakForce)` in Lua (the
  joint component is then removed); Inspectors and Scene-view gizmos (axes, limit arcs, ranges).
- Physics Material assets (`.physicmat`: dynamic and static friction, bounciness, combine modes)
  with a Material slot on every collider; static friction while resting, dynamic while sliding.
- Ragdoll Wizard (GameObject ▸ 3D Object ▸ Ragdoll…): bones auto-filled from the humanoid Avatar or
  bone names; capsules, boxes and a head sphere, body-segment masses, Character Joints with
  humanlike limits. A Ragdoll component switches animated ↔ physics while playing with a blend back
  to the animation; Lua `Ragdoll.Enable / Disable / IsEnabled`.
- XPBD cloth on skinned, plain and primitive meshes: stretch, bending, tethers, per-vertex max
  distance (painted in the Scene view or read from a vertex colour channel), sphere and capsule
  colliders, the Wind Zone, optional self-collision. One job per cloth per frame on the background
  pool (the main thread never waits), drawn through the GPU skinning hook.
- Scene view ▸ Physics Debug draw mode: colliders by body state, contacts, joints, sleeping bodies,
  cloth particles, with a legend.
- PhysicsDemo2 sample: a ragdoll dummy you knock over with thrown balls (R to get up), a flag in the
  wind, a bridge an anvil snaps, ice and rubber on a ramp.

### Not done yet
- Cloth collision with boxes, meshes and terrain, continuous collision, tearing, a GPU solver;
  ragdoll root following the body and get-up matching; Configurable Joint world-space frames,
  swap bodies, mass scales and a true slerp drive.

- No ETC2/ASTC fallback, no BC6H skies, no Humanoid rigs, LODs not cached in Library. (Tree, grass/flower and
  terrain-layer textures: done in the follow-up below.)

## Round 18D follow-up — compressed tree, grass and terrain textures (branch tex-compress2)

### Done (docs/pending/r18-assets.md, "Tree, grass and terrain textures")
- The renderer's own textures use the BC blocks too: tree bark / leaf maps and impostors, the grass atlas with
  its flower layers, the terrain layer arrays (`sprites/compress/generated.rs`). Formats by role from the main
  source file's Compression: opaque terrain albedo BC1 (BC7 at High Quality); colour + alpha (bark + height,
  leaf / grass / flower cutouts, impostors) BC7; packed data (normal x, y + roughness, occlusion,
  translucency or height) BC7, four channels in one texture.
- Coverage-keeping mips are kept (encoded level by level); BC7's edge-alpha bias is taken out of cutouts; a
  30 dB quality gate keeps data BC7 cannot hold uncompressed (MeadowDemo's meadow thatch normal + height).
- Cached in `Library/TextureCache` by packed pixels and recipe; encoded in the background in the editor with
  the RGBA8 textures drawing until the blocks are ready; baked into builds from the packed `.tree` and
  `.terrain` files; players read the cache only.
- MeadowDemo: renderer-made textures 218.1 → 68.5 MB of GPU memory (trees 139.8 → 35.0, terrain layers
  44.7 → 25.2, grass and flowers 33.6 → 8.4); all GPU textures 403.3 → 253.7 MB. GPU frame time at 1080p High 3–4 % lower (wide 16.00 → 15.51 ms, behind 17.28 →
  16.59 ms, oak 14.66 → 14.16 ms). Golden shots within tolerance (meadow mean 0.15–0.39, changed ≤ 0.03 %), references
  not updated.

### Not done yet
- The terrain normal + height array of MeadowDemo stays RGBA8 (BC5 + BC4 would need another binding); identical
  textures of different tree assets are uploaded once per asset.

## Round 19B — Navigation: NavMesh parity (branch r19-nav)

### Done (docs/pending/r19-nav.md, screenshots `docs/screenshots/r19/nav_*`)
- Tiled voxel nav mesh: per-tile heightfield with a border, erosion, monotone regions, traced and
  simplified contours, convex polygons (up to 6 corners) with detail heights (edges and interior);
  64-voxel tiles on a world-anchored grid, portals inside and between tiles. Bakes run tile by tile
  in parallel on the background pool; unchanged tiles are reused (input hash).
- NavMesh Surface (agent type, collect All / Volume / Hierarchy, layers, voxel and tile overrides,
  Bake / Clear with progress), NavMesh Modifier, NavMesh Modifier Volume, NavMesh Link, carving
  Nav Mesh Obstacle (move threshold, time to stationary, carve only stationary) rebuilding only the
  touched tiles in the background while playing; `BuildNavMesh` / `UpdateNavMesh` from Lua.
- 32 areas with costs (Navigation ▸ Areas), per-agent area masks and costs; A* over polygons and
  links, funnel string pulling plus raycast shortcuts. Several agent types (Navigation ▸ Agents),
  each with its own data. ORCA avoidance with priorities; off-mesh link traversal (auto or
  `CompleteOffMeshLink`) with `OnOffMeshLinkStart/End` callbacks; `NavMesh.Raycast`,
  `FindClosestEdge`, `SamplePosition`, `CalculatePath` with masks.
- Scene view: nav meshes coloured by area, tile borders, link arcs, live carved meshes and agent
  paths in Play mode. Sample `NavigationDemo2` (crowds, Mud area, jump link, carving Rolling
  Block, Humanoid and Ogre). `.navmesh` format 2 (format 1 still loads).
- Measured (release, this machine, `nav_perf_report`): a 160 m bumpy terrain with 80 boxes
  (206k triangles, 0.167 m voxels) bakes into 256 tiles / 1117 polygons in 0.14 s; an unchanged
  rebake reuses every tile in 0.02 s. A carving obstacle settling rebuilds one tile in about 5 ms
  (background pool). Agents: SetDestination
  0.006 ms each; update 0.52 ms per frame for 100 agents, 4.0 ms for 400 (High Quality avoidance).

### Not done yet
- Watershed regions, automatic drop / jump links, links across surfaces, a compressed tile cache
  for carving (tiles rebuild from collected geometry), physics colliders as bake geometry,
  `SamplePathPosition`.

## Round 19E — Timeline and cameras: cutscene tooling (branch r19-timeline)

### Done (docs/pending/r19-timeline.md, screenshots `docs/screenshots/r19/timeline_*`)
- Camera track: clips show a virtual camera, overlapping clips blend (the timeline's mix), eases
  blend with the brain's live camera; in Play mode the brain is overridden per frame and blends
  back to its live camera when the cutscene ends (blend list first). Clips point at scene objects
  through the director's new `references` (remapped like bindings).
- Control track: activation (Post-playback State), sub-timelines (local time = clip in + (t −
  start) × speed, hold or loop, nested 8 deep, their markers fire at the mapped time), particles,
  audio and Impulse Sources while playing.
- Record mode (⏺ on Animation tracks: any change to the bound object becomes a key at the
  playhead, restored when the preview ends), Ripple editing (move, trim, delete), edit-mode preview
  of Camera tracks posing the Game camera (transform and lens) and putting it back.
- Signals: `signalReceived` listeners on the director as well as the marker's script method.
- Virtual cameras: three-ring Free Look (top / middle / bottom, spline curvature, gizmo) with
  Input System action axes; Impulse Source (Bump / Explosion / Rumble, envelope, radius,
  dissipation, channels, collision trigger) and Impulse Listener; Target Group (weights, radii)
  with a Group Composer that dollies and / or zooms to keep the group in view; Custom (Bézier)
  blends and `.blends` blend list assets; Confiner (box / sphere collider volumes).
- Lua: `SetGenericBinding` / `GetGenericBinding`, `played`, `signalReceived`; `xAxis` / `yAxis` /
  inputs; `ImpulseSource:GenerateImpulse`, `Impulse.GenerateImpulse(At)`; Target Group members.
- Sample `CutsceneDemo`: an 18 s cutscene with cuts and blends, a group shot of three characters,
  an explosion signal shaking every camera, a sub-timeline flicker, then a custom-curve blend to a
  Free Look camera.

### Not done yet
- Recording into `.anim` clips, Replace edit mode, nested audio tracks, Control prefab instancing,
  particles in the edit-mode preview, dolly / path cameras, signal assets with a receiver
  component.
## Round 19D — Audio: mixer routing and spatial depth (branch r19-audio)

### Done (docs/pending/r19-audio.md, screenshots `docs/screenshots/r19/audio_*`)
- Our own mixer graph (`audio/engine.rs`) replaces kira's tracks; kira only opens the device and
  plays the graph as one sound. The same graph renders offline, so tests check real buffers
  without a sound device. Commands go through a lock-free ring; levels, gain reduction,
  positions and finished voices come back with `try_lock`.
- Send / Receive / Duck Volume units (side-chain ducking with threshold, ratio, attack, release,
  knee, sidechain mix); tracks processed in dependency order so sends land in the same block.
- New effects Chorus, Flange, Pitch Shifter (phase vocoder), ParamEQ; Lowpass, Highpass, Echo,
  Reverb, Distortion and Compressor reimplemented on the graph. Every effect has an editable
  chain position (drag or Move Up / Down), bypass and a wet mix level.
- Live edits: inserts fade in, removals fade out with their tails, reorders crossfade from the
  old chain, reroutes and moved voices feed both destinations for 20 ms; no source restarts;
  voices of a removed group move to its nearest surviving parent.
- Nested mixers (a mixer's Output is another mixer's group; loops refused) and weighted snapshot
  blends (`TransitionToSnapshots`).
- Spatialize: spherical-head binaural model (ITD up to 0.66 ms, head-shadow shelf, rear cue);
  spread moves sideways cues to the front; Custom rolloff curves with an Inspector curve editor.
- Clips: Decompress On Load / Compressed In Memory / Streaming (worker-thread decoding through
  symphonia, already in the tree via kira; Vorbis was already supported so no new decoder),
  Force To Mono, import settings and format info in the clip Inspector.
- Lua: `TransitionToSnapshots`, mixer `outputAudioMixerGroup`, `spatialize`, Custom rolloff with
  `SetCustomCurve` / `GetCustomCurve`; any effect parameter, send level or wet level can be
  exposed for `SetFloat` / `GetFloat`.
- Mixer window and Inspector: send targets, "Sends from", ducking bars and readout in Play mode,
  filter / EQ response curves, mixer Output picker. Sample `AudioDemo2` (ducking by a send,
  reverb send zone, Outdoor / Indoor snapshot blend, HRTF orbit, streamed score, custom rolloff).
- Measured (release, this machine, `mixer_cost_report`, 48 kHz): per voice 0.92 ms of audio-thread CPU per second of sound (0.09 % of one core) for a 2D voice,
  1.08 ms (0.11 %) resampled, 2.15 ms (0.21 %) spatialized (HRTF); 32 HRTF voices ≈ 6.9 % of a core.
  Per effect instance: Lowpass / Highpass / ParamEQ 0.24 ms (0.02 %), Echo 0.48 ms, Duck Volume
  0.52 ms, Compressor 0.74 ms, Distortion 1.34 ms, Flange 1.70 ms, Reverb 1.96 ms (0.20 %),
  Chorus 2.33 ms (0.23 %), Pitch Shifter 27.9 ms (2.8 %, FFT 1024 / overlap 4); graph overhead
  0.35 ms per second.

### Not done yet
- Measured HRTF sets and elevation cues; transcoding at import (Vorbis / ADPCM encoders);
  Preload Audio Data / Load In Background; mixer views and effect presets; spread and
  reverb-zone-mix curves over distance; sends between mixers.
## Wireframe draw modes on the GPU (branch wireframe)

### Done (`render/wireframe.rs`, screenshots `docs/screenshots/gfx/wireframe_*`)
- The Scene view's Wireframe and Shaded Wireframe now show everything the GPU draws, not just ordinary meshes:
  each part draws its triangles a second time as edges with its own vertex stage and the culling and LOD the
  frame picked: meshes (each visible draw, both faces), terrain chunks (their LOD index ranges), trees (bark,
  leaf cards, impostor quads far away), grass and flowers (6 % of the standing blades within 10 m, thinning
  out over the last 5 m, like Unity's sparse detail meshes) and virtual geometry clusters (both culling phases).
- Edges come from polygon mode Line (`POLYGON_MODE_LINE`, requested when the adapter has it); without it, from
  filled triangles that keep the pixels within half a pixel of an edge (`SHADER_BARYCENTRICS`); without
  either, the editor keeps its CPU edge lists with the 2M-edge budget. `MISTY_WIREFRAME=cpu|barycentric`
  forces a fallback.
- Shaded Wireframe: edges at the end of the main pass (before the gizmo lines), depth tested against the
  scene with a small bias, in the old colour (black, 35 %). Wireframe: the geometry renders unlit, without
  lights, shadows, water, particles, transparents or post-processing, for its depth only; a flat background
  (the view's grey) is painted over its colour, then the grid and the edges (dark grey, 90 %). Hidden lines are
  removed by the real surfaces (alpha-tested leaves and blades included), which keeps MeadowDemo readable.
- The edges' opacity rides in the view uniform's unused `view_tan.w` and their colour is the pass's blend
  constant, so the shared shader prelude stays byte for byte as it was: changing it made every pipeline new to
  the driver's shader cache (MeadowDemo's first frame 0.3 → 2.5 s in the hitch test). Shaded modes are
  untouched.
- Cost, MeadowDemo wide at 1920×1080, High (`wireframe_cost`): shaded 15.3 ms GPU / 2.0 ms CPU; Shaded
  Wireframe 16.1 ms / 2.1 ms (+0.8 ms; +0.5 ms with virtual geometry on); Wireframe 6.6 ms / 0.8 ms (no
  lighting, shadows or post-processing).
- Tests: `wireframe_meadow_draws_terrain_edges` (both modes, virtual geometry off and on, no validation
  errors, edges over the ground, flat background above the horizon) and the barycentric fallback on a device
  that has barycentrics.

### Not done yet
- The Wireframe mode still pays for the geometry's shading passes (unlit); a depth-only path would be
  cheaper. Water and particles show no edges. Shaded Wireframe edges go through TAA like the gizmo lines.

## Round 20E — Profiling and debugging depth (branch r20-profiler)

### Done (docs/pending/r20-profiler.md, screenshots `docs/screenshots/r20/profiler_*`)
- Profiler GPU Usage module: per-pass GPU timestamps read back without waiting (a ring of query
  sets, the renderer's existing pass marks), stacked by pass group, GPU Hierarchy per view;
  whole-render or "not supported" fallback.
- Physics, Audio and UI modules with per-frame counters and times (bodies, contacts, joints;
  voices, mixer groups, effects, streams, audio-thread mixing time and DSP load; canvases,
  elements, batches, vertices).
- Timeline rows: Main Thread, the GPU passes of each render, the audio thread, the background
  pool and rayon workers.
- Capture Save / Load (`.mprof`), Target ▸ Player: a Player with
  `MISTY_PROFILER_CONNECT=127.0.0.1:54998` streams its frames to the editor.
- Frame Debugger: Draw Indirect events (GPU-driven culling, virtual geometry, grass, VFX),
  Bindings card, Depth / Normals / GI / Emission views, Draws only stepping.
- Memory Profiler: GPU and CPU memory by category, CPU asset caches (model meshes and
  embedded textures, runtime textures, mixer clips, mesh distance fields).
- Overhead on MeadowDemo: see docs/pending/r20-profiler.md (recording and closed).

### Not done yet
- Per-pass GPU times without timestamps inside encoders; visible instance counts of indirect
  draws; Player discovery / autoconnect; live resource views in the Bindings card.
## Round 20C — Deeper VFX: GPU graph features and Particle System modules (branch r20-vfx)

### Done (docs/pending/r20-vfx.md, screenshots `docs/screenshots/r20/vfx_*`)
- Effect Graph: GPU back-to-front sorting (bitonic, sized by the live count; Sort Auto / On /
  Off per output), Mesh outputs (built-in primitives, lit), Trail outputs (a ring of past
  positions per particle) and Ribbon outputs (one strip, youngest first), Collide with Depth
  Buffer (the last scene depth copy and its camera), GPU Event systems (On Die / On Collide,
  count per event, Inherit Source Velocity / Size) and operator sub-graphs (`.vfxoperator`,
  flattened before compiling, errors on the node, live recompile of users).
- Particle System: MinMax curves (four modes) and MinMaxColor gradients everywhere the
  familiar engine has them; Velocity / Limit Velocity / Force over Lifetime, Color by Speed,
  Size by Speed, Rotation over Lifetime / by Speed, Noise, Collision (planes and world
  colliders), Sub Emitters (birth / collision / death), Texture Sheet Animation, Trails
  (particles and ribbon), stretched / horizontal / vertical billboards; Inspector sections for
  each. Older scenes keep their exact text.
- Sample `VfxDemo2`: fireworks, rain splashing off the depth buffer, lit debris, a sub-graph
  ribbon, sorted flipbook smoke, and Particle Systems with noise, trails, sub emitters and
  world collisions.
- Measured (`vfx_cost_report`, 1080p, GPU lock): 100k GPU particles alpha-sorted every frame
  with depth collision cost 0.50 ms of GPU simulation + sort (sort ≈ 0.34 ms, depth test
  ≈ 0) and +2.1 ms on a whole frame over the floor alone (drawing ≈ 0.8–1.0 ms, the depth
  copy collision needs ≈ 0.7 ms).

### Not done yet
- Imported-mesh outputs, sub-graph blocks, strip ids for several ribbons, colour in GPU event
  records, off-screen depth collision; CPU Inherit Velocity / Lights / External Forces / Custom
  Data modules, more Shape types, smooth curve tangents.

## Round 20B — Terrain tools at Unity parity, placement brushes (branch r20-terrain)

### Done (docs/pending/r20-terrain.md, screenshots `docs/screenshots/r20/terrain_*`)
- **Paint Holes**: holes are cut per heightmap cell and saved in the `.terrain` asset. Chunks with
  holes draw from their own index lists at full detail, so every render pass is cut with no
  shader change. Hole cells are removed from the rapier heightfield, rays fall through them,
  and nothing grows over them.
- **Up to 16 layers**: four to a splat map, the extra maps in `alphamap_extra` (empty with four
  layers or fewer, so those terrains save and draw as before). Texture arrays get one slice per
  layer. A per-chunk layer mask makes a pixel read only its chunk's maps and height-blend the
  four strongest layers. Brush, Auto Paint, Generate in brush, harmony and grass rules all work
  across maps.
- **Paint Details**: the terrain grass plus up to 16 detail layers, each with a painted density
  map. Grass species (height, width, bend, colour, seed heads) ride in the terrain grass's
  chunks and GPU path. Detail meshes are instanced mesh draws with random scale and turn,
  optional align-to-normal, culled by screen size.
- **Placement**: Paint Trees gains a slope range, height limits and random rotation. **Place
  Objects** brushes prefabs (rocks, props) onto the terrain as scene objects, with density,
  spacing, scale range, rotation, align-to-normal, slope and height limits, and erase (all or the
  selected prefab only). Each stroke is one undo step.
- **World Builder ▸ Scatter Storage ▸ Terrain Trees**: tree items become editable terrain tree
  instances (a per-item Terrain Tree asset, with the builder's own Oak / Birch as the default).
  Regenerating replaces only those trees.
- Sample `TerrainToolsDemo` (seven layers over two maps, sinkhole over a pit, two grass species,
  pebbles, trees, placed rocks).
- MeadowDemo unchanged: all 20 golden shots match (mean 0.15–0.39, ≤ 0.03 % changed, same as main).
  `meadow_perf_times` at 1080p, baseline (7d17100) vs this branch, two alternating rounds: wide
  High 15.47 / 15.46 ms GPU, Medium 11.39 / 11.40, Low 7.65 / 7.66; behind High 16.60 / 16.62,
  Medium 11.79 / 11.81, Low 7.72 / 7.73. Same triangles and draw calls; CPU render time within
  noise (1.6–2.2 ms both).

### Not done yet
- Sub-cell hole masks. A grass species's own colour and wind (it picks a point between the
  terrain grass's healthy and dry colours). Billboards, wind and GPU scatter for detail meshes.
  Storing placed objects in the terrain asset. World Builder keeping holes and detail layers
  when it regenerates.

## Round 20D — Shader Graph depth: production graphs (branch r20-shadergraph)

### Done (docs/pending/r20-shadergraph.md)
- **Sub Graphs** (`.shadersubgraph`: properties are inputs, Output node slots are outputs; inlined
  in code generation, CPU previews too, listed in Create Node), **keywords** (Boolean and Enum on
  the Blackboard, Keyword node, each combination a variant with only its nodes, capped at 128;
  materials pick the variant with a checkbox or dropdown; `.shader` files get `@keyword` and
  `#if` blocks), **Custom Function** (inline WGSL body or a `.wgsl` file, any inputs and outputs,
  Node Settings card), **groups** (Ctrl+G, drag, rename), copy / cut / paste / duplicate across
  graphs (window clipboard plus system clipboard text, properties and keywords merged), and a
  **save prompt** when closing the tab, opening another graph or quitting with unsaved changes.
- Inputs: real **vertex colours** (glTF `COLOR_0` imported, a second vertex stream for custom
  shaders, white otherwise) in both stages, with the Vertex block Position Offset; a Triplanar
  node.
- Master stack: Ambient Occlusion, Transmission and a Normal Space setting on top of the lit
  inputs; a **Decal target** whose graph runs inside the decal pass for Decal Projectors with a
  graph material (new projector Material field).
- Compile errors land on their node (Custom Function bodies, Sub Graph internals on the Sub Graph
  node, the variant cap on a Keyword node).
- Examples in `SampleProject/Assets/Examples/Shader Graph`: Dissolve Glow (+ Dissolve Edge Sub
  Graph, 4 variants), Triplanar Rock, Wind Sway (vertex-coloured glTF grass), Puddle Decal, and
  `ShaderGraphExamples.scene`; tests compile every variant and render the scene on a GPU.

### Not done yet
- Clear coat / anisotropy / detail / parallax outputs wait for the 17D material model (not in
  main); no CPU preview for Custom Functions; vertex offsets do not move shadows; decal graphs
  limited to 8 properties; FBX / OBJ / skinned meshes have no vertex colours yet.

## Round 21A — Build Profiles, dedicated server, achievements, build report (branch r21-buildprofiles)

### Done (docs/pending/r21-buildprofiles.md)
- **Build Profiles** window (File ▸ Build Profiles, Ctrl+Shift+B) in place of Build Settings:
  Windows, Linux, macOS and Dedicated Server platform profiles plus custom profiles (add,
  duplicate, rename, delete), each with its own scene list or the shared one, scripting defines,
  texture compression and max size overrides, a quality level, Development / Compress / Zip and
  an output folder. Saved as `.buildprofile` assets (`ProjectSettings/BuildProfiles/`,
  `Assets/Settings/Build Profiles/`); one is active for Ctrl+B and Play mode. CLI:
  `misty --build <project> [<out>] --profile <name>`.
- Lua scripting defines: `--#if NAME` / `--#elif` / `--#else` / `--#endif` blocks and
  `Application.HasDefine`, `Application.isDedicatedServer`, `Application.isEditor`.
- **Dedicated server**: the player runs headless (no window, audio device or GPU) for Dedicated
  Server builds or `--headless`, starts the network server (`--port`) and ticks at the Target
  Frame Rate; textures, audio, video, compressed textures and the icon are stripped from server
  builds. Tested end to end: a server build of the real player plus a headless client process.
- **Achievements**: Lua `Achievements.Unlock` / `IsUnlocked` / `Clear` / `GetUnlocked` over a
  pluggable `AchievementBackend`; local JSON storage (editor Library, player user folder).
- **Build report**: sizes per asset type and the 20 largest assets, shown in the window after a
  build, saved as `BuildReport.txt` in the build and `Library/BuildReports/<Profile>.json`.
- Sample profiles in SampleProject: Demo and Net Server.

### Not done yet
- No Steam backend for achievements: there is no pure-Rust, permissively licensed Steamworks
  client (the crates wrap Valve's proprietary SDK). The backend trait is where it would plug in.
- The server is the same player binary run headless, not a separately compiled player without
  winit / wgpu. Texture overrides don't reach the textures the renderer makes from several
  images (terrain arrays, tree atlases). Profiles have no per-profile Player Settings overrides.
## Round 21E — Version control, API reference, accessibility (branch r21-ecosystem)

### Done (docs/pending/r21-ecosystem.md)
- **Version control** through the `git` command line (no libgit2): the project's repository is
  found with `rev-parse` (a project inside a bigger repo works), status is polled every 3 s on a
  worker thread, and Project files / folders and the Hierarchy's scene rows get Modified / Added /
  Untracked / Conflicted badges. **Window ▸ Version Control…**: staged / unstaged / conflict
  lists with a checkbox per file, coloured diffs of text assets, a commit message and Commit,
  Stage All, Discard with a confirmation (new files go to the trash). No push or pull buttons:
  the commands are shown with Copy buttons.
- **Scene diff**: objects keyed by Hierarchy path, references compared by object rather than by
  list index; added / removed / changed objects, components and every changed field, plus lighting
  and order changes. **Scene merge helper** for a conflicted scene: each object takes ours or
  theirs (one-sided changes preselected), Save Merged Scene writes and stages it. Tests use temp
  repositories only (git refuses folders outside the temp folder in test builds).
- **Lua API reference**: `misty --api-docs <folder>` and Help ▸ Scripting Reference generate
  Markdown and one static HTML page from the binding sources (`scripting/*.rs`, scanned for
  tables, userdata members, closure parameters and the comment above each binding) and the
  autocomplete table. `docs/api/` is committed; a test fails when it is stale.
- **Accessibility**: AccessKit screen-reader support named in the manifest, new controls labelled
  and an audit helper (`a11y::unnamed_controls`) checked on the toolbar, Hierarchy, Project,
  Preferences, Version Control and Player settings; Edit ▸ Preferences ▸ UI Scale (75–200 %,
  per user). Runtime: Project Settings ▸ Player ▸ Accessibility with a UI text scale and a
  colour-blind filter (protanopia / deuteranopia / tritanopia correction or simulation, strength)
  applied over the final image including the UI, in the player and the Game view; Lua
  `Accessibility.textScale / colorBlindMode / filterStrength / simulate`. Off costs nothing, so
  golden images are unchanged.

### Not done yet
- Hunk staging, history and branch views; folder-tree badges; per-field scene merges and prefab
  object diffs; accessible names for the older Inspector rows.
## Round 21 (17D) — Standard Lit v2 material model (branch r21-material)

### Done (docs/pending/r21-material.md)
- Optional Standard features, each a shader variant bit so unused ones cost nothing (a
  `MAT_FEATURES` constant in mesh.wgsl, 0 in every base shader; feature materials get a variant
  for exactly their bits and a bigger group 2): **detail maps** (×2 albedo, normal, mask, own
  tiling), **parallax occlusion mapping** (height map, scale, steps), **clear coat** (strength,
  smoothness, optional normal), **translucency** (wrap lighting + transmission, colour,
  thickness map), **anisotropy** (strength, Horizontal / Vertical / Radial direction,
  rotation), and a dielectric **specular** tint.
- `.mat` sections left out at their defaults (old materials re-save byte for byte); Material
  Inspector sections with Unity-style checkboxes; Sphere / Cube / Plane preview.
- glTF: `KHR_materials_clearcoat`, `_anisotropy`, `_specular`, `_diffuse_transmission`,
  `_transmission` (+ `_volume` tint) and `KHR_texture_transform`.
- Shader Graph: Clear Coat, Anisotropy and Subsurface blocks (Graph Settings toggles); surface
  shaders writing `o.coat` / `o.anisotropy` / `o.subsurface` compile those features.
- Lua: Unity property names (`_ParallaxMap`, `_DetailAlbedoMap`, `_CoatMask`, `_Anisotropy`, …)
  and feature keywords (`_CLEARCOAT`, `_PARALLAXMAP`, …).
- MaterialDemo2 sample (car-paint clear coat, brushed metal disc and pipe, parallax brick wall,
  detail rock, translucent leaves; procedural textures).

### Not done yet
- Textures of the glTF extensions other than the clear coat normal (coat / roughness masks,
  anisotropy direction map, thickness and transmission maps); real refraction for
  `KHR_materials_transmission`; parallax doesn't offset depth or self-shadow; area lights use the
  plain lobe; Shader Graph has no Parallax Occlusion Mapping node yet.

## Round 22 — project templates (branch r22-templates)

### Done (docs/pending/r22-templates.md)
- New Project offers four templates: **MeadowDemo** (unchanged), **First Person** (endless white
  plain, the First Person Player prefab, its head camera as the Main Camera), **Racing** and
  **Floating Platform** (the old Standard, now with post-processing). **PP Ready** (the ocean
  scene) is removed; `--new standard` still gives Floating Platform.
- Every template starts with a global volume: AgX tonemapping, bloom, volumetric clouds and sun
  shafts (MeadowDemo already had them; a test checks all four templates).
- **Racing**: a 1.2 km closed spline circuit (Road extrude, red and white kerbs, start gantry and
  chequered line, marker posts, tyre barriers in the corners, trees, grandstand, hills), a car on
  four Wheel Colliders with three primitive bodies (Sport, Muscle, Kart), and Lua scripts in
  `Assets/Scripts`: RaceCar (arcade grip, downforce, top speed, boost, reset to the track),
  ChaseCamera, LapTimer and RaceSettings (car, gravity, mass, grip, top speed on keys shown in a
  HUD panel). Generated textures only.
- Lua: `Physics.gravity` (get/set), settable `Rigidbody.mass`, `WheelCollider.forwardStiffness`
  and `sidewaysStiffness`.
- Hub cards, thumbnails rendered from the scenes, README and release notes updated; tests create,
  open and play every template, and drive the racing car round 40 % of the loop headlessly.

### Not done yet
- Banked curves (needs a Mesh Collider on the runtime spline mesh); the three car bodies share
  one wheelbase and collider; no opponents, sound or gamepad in the Racing scripts.
## Round 21 — World Partition-lite: world streaming (branch r21-streaming)

### Done (docs/pending/r21-streaming.md)
- **Cells as sub-scenes**: a World Partition component (cell size, grid origin, folder, load /
  unload radius, HLOD distance, frame budget, parallel loads, cell list); **Convert Scene** splits
  the persistent scene's objects into `<folder>/Cells/Cell_x_z.scene` (Always Loaded objects,
  cameras, lights, UI and other placeless objects stay); converting again re-splits; Save Cells.
- **Streaming**: Streaming Source component (main camera by default), load / unload radii with
  hysteresis; cell files are read, parsed and their terrain data, models, Mesh Collider shapes
  and material textures read ahead on the background pool; the main thread adds and removes
  whole top-level objects within the per-frame budget (entity ids reserved up front, so
  references survive slicing), and the Runtime starts their physics and scripts in one batch per
  frame (cost measured and kept free in the budget). Runs in Play mode and the player; Edit mode
  loads by hand or around the Scene camera.
- **HLOD**: Build HLODs merges each cell (coarsest LOD levels simplified, terrains as 32 m grids
  coloured from the splat layers, tree crowns) into one vertex-colour mesh per cell
  (`<folder>/HLOD/*.hlod`, a few kB) drawn for unloaded cells and hidden once the cell is in.
- **Terrain tiles**: tiles from a height function or split from a big terrain, stitched (shared
  edges, a saved one-sample border so edge normals match) and LOD-stitched across tiles by the
  renderer: no cracks, no lighting seams. GPU data of tiles that went away is dropped.
- **Editor**: World Partition window (minimap by cell state, click / double-click to load and
  unload, actions, stats and a frame graph), inspectors, Scene view gizmos (cells by state,
  source radii) and a stats overlay in the Scene and Game views.
- **Lua** `WorldPartition`: loaded / pending / memory / timing fields, `IsAreaLoaded`, `CellAt`,
  `GetCellState`, `LoadCell` / `UnloadCell`, `WaitForArea` (coroutine), `OnAreaLoaded`.
- **StreamingDemo** (4 × 4 km, 64 cells of 512 m: tiles, forests, boulders, HLODs; the camera flies
  a loop in Play mode), generated by `sample_streaming.rs`, 4.7 MB on disk.
- Player builds (Build Profiles) pack the cells, proxies and their material (the dependency walk
  reads the World Partition's cell list).
- Measured flying the demo's 5.1 km diagonal at 120 m/s: streaming main thread max 1.98 ms with
  no frame over the 2 ms budget (CPU bench, full game loop); rendered at 1280×720 High, no frame
  above 16 ms (max 12.75 ms, frames where cells finished loading max 12.04 ms) and 0.3 % of frames
  over the streaming budget by at most 0.7 ms. Golden shots unchanged.

### Not done yet
- First GPU uploads of new meshes and tree instance lists stay on the render thread; HLOD trees
  are diamonds, not impostor cards; shadow-pass LODs aren't stitched across tiles; no per-cell
  dirty tracking in Edit mode; nav meshes / lightmaps / occlusion not split per cell; no
  velocity look-ahead for streaming sources.
## Round 22 — Performance (branch r22-perf)

### Done (docs/pending/r22-perf.md)
- Profiled MeadowDemo per pass (wide, close, behind × Ultra / High / Medium / Low) and the
  editor's frame; a new test-build switch, `MISTY_SHADER_PATCH`, rewrites shader sources at
  pipeline creation so each pass's parts were costed without rebuilds.
- Same picture, less work: sharp sun shafts read the cascade whose texels the march can resolve
  (Ultra shafts 2.15 → 1.3 ms); the grass patch / worn-spot noise skips octaves that cannot
  change the result (terrain −0.16 to −0.34 ms on every level, the CPU blade scatter too); tree
  leaves draw their depth first and shade each sample once (trees 1.25 → 0.41 ms in the behind
  view on High); geometry nearer than the cloud base skips the cloud lookups; shadow lists of
  small frames no longer wait on rayon workers (7 ms average render-CPU stalls on a busy
  machine gone).
- Editor Profiler scopes per panel and Scene view stage. The editor's Scene view is CPU bound
  (15.6 ms frame for a 6 ms GPU frame): submit, scene gathering, gizmos and file polling.
- Material model v2 checked against main before its merge: no cost on plain materials.

### Not done yet
- Editor CPU: `Render.Submit` (3 ms), Scene view gathering every frame (1.3 ms), file polling
  on the main thread (spikes every 0.5 s), gizmo lines (0.6 ms).
- Grass visibility / shading (3–4 ms together) and the terrain's lighting stay the biggest GPU
  costs; look-changing options for the user are listed in the pending notes.

## Round 22 — visual scripting (branch r22-visual-scripting)

### Done (docs/pending/r22-visual-scripting.md, guide: docs/visual-scripting.md)
- **Script Graphs** (`.vgraph`): node-based logic like Blueprints / Unity Visual Scripting,
  attached through the Scripts list like Lua scripts (Inspector shows the Object variables).
  White exec wires and typed data wires; pure nodes are lazy.
- **Runtime in Rust**: graphs compile to a compact bytecode over per-instance registers (no Lua
  generated); events Start / Update / FixedUpdate / LateUpdate / Enable / Disable, triggers and
  collisions, Input System actions (performed / held / released), custom events (send,
  broadcast, receive; to and from Lua), timers, Delay, OnDestroy.
- **Nodes**: Branch, Sequence, For Loop, For Each, While (guarded), Do Once, Gate, Flip Flop,
  Delay; math / logic / vector / quaternion / random / string / list; Graph and Object
  variables; engine API nodes generated from the Lua binding table (common members native, the
  rest through Lua); Call Lua Function; subgraph assets and macros (inlined).
- **Script Graph window**: category search (filtered when a wire is dropped on empty canvas),
  type-checked wires, error badges, Blackboard, Graph Inspector, groups, notes, copy / paste,
  Collapse To Macro; Play-mode debugging with glowing exec wires, values on hover, breakpoints,
  Step and Continue. Shared node-canvas helpers now also used by Shader Graph and Effect Graph.
- Lua: `gameObject:SendCustomEvent`, `GetVariable`, `SetVariable`.
- Sample `VisualScriptingDemo` (door, coins + score UI, patrol on a timer, jump pad, player —
  all graphs, no Lua) with generator, `matches_its_generator` and a headless play test.

### Not done yet
- Hot reload of graphs while playing; graph state in save games; debugging inside macros;
  typed pins for Lua-backed API nodes; the Scratch-like block mode (design note in the guide).
## Round 23 — Editor CPU frame (branch r23-editor-cpu)

### Done (docs/pending/r23-editor-cpu.md)
- Repeatable editor bench: MeadowDemo Scene view with a fixed camera (capture runs print the
  p99 frame time now) and the flythrough, plus new Profiler scopes inside the Scene view's
  gather and `Render.Submit`.
- Live reload's file checks read stamps a background thread keeps fresh
  (`misty_engine::file_watch`); a stamp that differs is checked on disk before anything
  reloads. The 10–25 ms frame every half second is gone (worst pre-frame 23 → 0.9 ms).
- Gizmo lines only for selected objects; the Scene view's per-frame lists skip whole-Hierarchy
  walks for components the scene doesn't have, and the draw cache keeps unchanged entries
  (gather 1.2–1.5 → 0.3 ms, overlays 0.2 → 0.05 ms).
- Project window: off-screen tiles and rows paint nothing, no folder-tree copy per frame,
  search results reused while the text stays the same.
- Result: the editor's main-thread work in `ui()` 6.2 → 5.0 ms, frame p99 19.9 → 11.3 ms with a
  fixed camera; the Scene view is GPU bound now (frame ≈ GPU frame, fixed camera and flythrough).

### Not done yet
- `Render.Submit` (wgpu's command encoding, 1.7–2.0 ms) still runs on the main thread: a worker
  thread version saved nothing because the Scene view is the last panel drawn; it needs an
  egui-wgpu paint-callback hand-off. The GUID watcher's rescan when assets appear still runs on
  the main thread.
## Round 24 — Turkish and Arabic (branch r24-locale)

### Done (docs/pending/r24-locale.md, docs/localization.md)
- Editor UI languages: English, Türkçe, العربية. `tr!("…")` keyed by the English text (falls
  back to English), `{name}` placeholders filled after translating, `#context` keys for words
  with two meanings; one TOML file per language in `crates/misty_editor/locales`, embedded.
- Edit ▸ Preferences ▸ General ▸ Language and the Hub's Settings page: saved per user in
  `hub.json`, applied on the next frame. `MISTY_EDITOR_LANGUAGE`, `MISTY_OPEN_MENU`,
  `MISTY_PREFERENCES` for screenshots.
- Translated: menus, toolbar, tab titles, Hierarchy, Project, Console, Scene view toolbar,
  Inspector header and the common components (Transform, Camera, Light, Mesh Renderer,
  Rigidbody, colliders, Audio Source, Script…), Add Component, the Hub (projects, new project,
  template cards, Learn, Settings), Preferences, the save prompts, the status bar.
- Arabic in egui: letters joined (`ar-reshaper`) and lines reordered (`unicode-bidi`) in one
  place (`i18n::shape`), brackets mirrored, idempotent, each Arabic stretch handed to egui's
  own shaper in logical order; built-in Noto Sans Arabic subset (SIL OFL, Segoe UI line
  metrics) first in the editor's fonts for Arabic characters. RTL layout: mirrored title bar and menus, right-aligned
  Inspector labels, Hub navigation on the right, mirrored Default layout.
- Games: the Noto Sans Arabic subset is a built-in fallback of every UI Text (shaped with
  rustybuzz), brackets mirrored in RTL runs; Text ▸ Mirror for RTL; locale-aware case (`to_upper` / `to_lower` / `to_title`,
  `{x:upper}` formatters, `Localization.ToUpper` / `ToLower` in Lua) with Turkish i/İ, ı/I.
- Sample `LocalizationDemo2` (English, Turkish, Arabic) with generator, `matches_its_generator`
  and a play test; a CPU test checks the shaped glyphs of "مرحبا".
- Tests: placeholder mismatches fail; `untranslated_keys_report` prints coverage per language.

### Not done yet
- The rest of the editor (Project Settings, the graph editors, most component editors beyond
  the common ones, tutorials' steps) is still English; the Hierarchy and Project trees,
  submenus and panel toolbars are not mirrored; text fields edit Arabic in logical order
  without shaping.
## Round 25 — Project window fixes (branch r25-project-fixes)

### Done (docs/pending/r25-project-fixes.md)
- Right-click anywhere in the Project window's content (grid, list, empty folders, gaps) and
  the folder tree's empty space opens the folder menu; Create ▸ in a tree folder's menu makes
  the asset there. Same for the Hierarchy's empty space and the Timeline's track headers.
- Inline renames keep the keyboard: stable ids, no layout jump, and a lost focus only ends the
  rename after a click elsewhere or a key. New assets and folders start renaming with the whole
  name selected, scrolled into view.
- Files and folders dragged in from the desktop are copied (recursively) into the folder under
  the pointer or the open folder and imported, with a drop outline; on the Hierarchy or Scene
  view models and prefabs are also instantiated.
- Show in Explorer opens the right folder (or selects the file) also with spaces in the path
  (raw `explorer` command line); macOS / Linux equivalents.
- Files added, removed or renamed by other programs appear without leaving the folder and are
  imported: listed folders' stamps come from the background file watch, which now also wakes
  an idle editor.
- New: press and hold 1.5 s on a Project item or Hierarchy object to rename it (a bar fills;
  moving cancels). Timeline track, Input Actions, Shader / Script Graph group renames that could
  never end (focus asked every frame) now end on Enter / click away.

### Not done yet
- Dropping OS files onto a Hierarchy row as children of that object (they go to the root);
  on Windows the drop target is the panel/folder last under the pointer (winit reports no
  pointer moves during an OS drag).
## Round 27 — The moon, moonlight and stars (branch r27-moon)

### Done (docs/pending/r27-moon.md)
- Lighting ▸ Environment ▸ **Moon** (Procedural and HDRI skies; on by default, left out of scene
  files while default): Position Follow Sun (Phase × 360° east of the sun on its daily circle),
  Manual (azimuth, altitude, Phase) or Date and Time (real sun and moon from a date, local time,
  UTC offset, latitude and longitude: Meeus low-precision ephemerides with the moon's
  parallax); Size, Brightness, Earthshine, Texture, Moonlight intensity / colour, Stars.
- The disc is a sphere lit from the sun's direction: correct terminator, waxing / waning,
  crescent to full from one texture; earthshine on the dark side; crater relief near the
  terminator from the built-in height map. Drawn in the skybox before clouds and fog are
  blended over it (clouds in front hide it, haze dims it), reddened low by the air mass, hidden
  below the horizon, over HDRI skies too.
- Night sky: below −1° the procedural sky blends to a dark blue night sky that brightens under a
  high full moon; sky ambient, reflections and the clouds' sky light follow. A fixed star field
  fades in behind the moon. Daytime skies are unchanged.
- Moonlight reuses the main Directional Light and its shadows: below the horizon the sun fades
  out (0° to −3.4°), then the light turns to the moon and fades in (to −11.5°), cool, scaled by
  Allen's phase law (dark at new moon) and the air mass. No pop, no second shadow map. The
  volumetric clouds and their shadow map take the moon as their light at night.
- Built-in texture made by `moon::texture_gen` (2048 × 1024, albedo + height: the real maria,
  power-law craters with rims, peaks, ejecta and rays, named landmarks), regenerate test; project
  images (2:1 equirectangular or a square near-side photo) override it.
- Lua: `RenderSettings.moon*`, `stars`, read-only `moonIllumination` / `moonDirection`.
- Sample `Scenes/MoonDemo.scene` (moonlit valley, standing stones, cabin, `MoonCycle.lua` runs the
  phases), generator + `matches_its_generator` + play test.
- Tests: phase fraction vs angle, phase law, Follow Sun placement, moonlight new vs full,
  smooth twilight, Meeus reference values and the 2024 Dallas eclipse, GPU disc phases.

- Night Sky section: star density, magnitude spread, glow, colour variation, twinkle, fade in a
  bright sky, Night Sky Darkness; a procedural Milky Way placed from the galactic pole and
  centre (core, dust lanes, width, tint); light pollution; an animated ray-marched aurora
  (colours, height, thickness, curtains, waviness, rays, direction, distance, speed, pulse,
  scene light); meteors. Stars and Milky Way turn with sidereal time. Lua for all of it, sample
  `Scenes/NightSkyDemo.scene`, GPU test.
- GPU cost of the night sky (`render::moon::gpu_tests::moon_gpu_cost`, whole frame, 1080p, GPU
  median ms, RTX 4060, two rounds):

  | Night sky | High, round 0 | High, round 1 |
  |-----------|---------------|---------------|
  | nothing | 0.848 | 0.800 |
  | stars | 0.836 | 0.842 |
  | moon | 0.804 | 0.807 |
  | Milky Way | 1.194 | 1.198 |
  | aurora (4 curtains) | 2.982 | 2.983 |
  | everything | 3.530 | 3.531 |
  | Low path: nothing | 0.800 | 0.801 |
  | Low path: everything | 2.224 | 2.219 |

  Stars and the moon are free within the noise, the Milky Way costs ~0.4 ms, the aurora ~2.2 ms
  on High (the Low path takes fewer steps and octaves: everything for ~1.4 ms).

### Not done yet
- No moon shafts or flare; no star catalogue; aurora not in water reflections;
  the air-mass code should merge with r26's sun-height intensity once both land.

## Round 28 — 2D Platformer template (branch r28-template2d)

### Done
- New Project ▸ **2D Platformer** (`--new "2D Platformer"`, also `2d` / `platformer2d`): built
  from the Platformer2D and TilemapDemo samples and generated in code (like Racing), so every
  install can make it, sample project or not. The pixel art, tiles and level layout now live in
  `templates/platformer_2d/art.rs`, shared with the two sample generators (their
  `matches_its_generator` tests still pass, so the committed samples are unchanged).
- The project: a daytime tilemap level on a Grid (Rule Tile ground with Tilemap Collider 2D
  and Shadow Caster 2D, animated water in a pit, ruined stone walls behind), the Platformer2D
  player (Rigidbody 2D, Box Collider 2D), ten coins counted by a UI Text, a sun travelling with
  the camera, clouds and hills, a Global Light 2D plus five torches with shadowed Point Light
  2D lights, and a bloom volume (sun, flames and coins glow). A Tile Palette
  (`Assets/Palettes/Level.palette`) holds the three tiles. Every asset gets a meta file with a
  GUID and the scene, tiles and palette carry GUID tables from the new project.
- Scripts in `Assets/Scripts`: `PlayerController2D.lua` (A/D or arrows run; Space, W or Up
  jumps; jump buffer, coyote time, short hop on early release; coins) and `CameraFollow2D.lua`
  (eases after the player, stops at the level's edges using the camera's size and the screen's
  aspect).
- Project settings for 2D: Sorting Layers Background / Default / Foreground (the sky, walls
  and water use them, with orders that also draw right before the layers are installed);
  2D gravity stays at the default (0, −9.81) with the player's Gravity Scale 3.
- The Scene view starts in 2D mode: `Library/EditorSettings.json` gained `scene_view_2d`
  (centre and half height), written by the template and remembered when a project closes in
  2D (new `SceneCamera::set_2d_view` / `view_2d`; turning 2D off returns to the 3D view).
- Hub card with a headless-rendered thumbnail (`assets/templates/platformer_2d.png`); Turkish
  and Arabic names and descriptions.
- Tests: the template's files, GUIDs, references, settings and scene objects; the editor
  opens it in 2D and reopens a moved 2D view; Play lands the player on the tiles, a held
  Space jumps high, a tapped W hops, Up jumps, running right with the arrow key collects three
  coins and the camera follows, without a script error; the Hub creates it from its card;
  the template names / aliases test and the GPU render test cover it.
- Screenshots: `docs/screenshots/r28/hub_new_project.jpg` (the new card), `platformer_2d_editor.jpg`
  (a new project: Scene view in 2D) and `platformer_2d_play.jpg` (Play mode).

- Checks: fmt clean, clippy 0 warnings; full tests: editor 811 passed, 3 failed under load
  (`mcp::tests::bridge_writes_and_removes_its_info_file`,
  `panels::packages::tests::registry_url_page_in_project_settings`,
  `scene_files::tests::opening_a_scene_lifts_a_view_left_under_the_terrain`; all three pass run
  on their own), engine 1384 passed, player 11, dedicated server and MCP e2e pass; golden
  shots 20/20.

### Not done yet
- No parallax or level end/goal; the Tile Palette window is not opened automatically.
- The stone wall tilemap shows faint vertical seams between some tile columns (looks like a
  tilemap chunk rendering artefact; not investigated this round).

## Round 29 — Painting grass shows at once (branch r29-grass-paint)

### Done
- Paint Details at 1000 blades/m²: the painted slice vanished and took minutes to come back.
  Every stroke frame dropped the chunks under the brush and restarted every scatter of the
  terrain, so nothing finished until the mouse was released; then each 256k-blade chunk was
  scattered on one thread and uploaded 8k blades a frame, behind the camera's chunks.
- Edits now mark chunks out of date instead of dropping them: the old blades draw until the
  new scatter is on the GPU (paint, erase, Fill / Clear, undo, settings sliders). An edited
  chunk has one job at a time; edits made meanwhile go out with its next job, and older
  results still show, so a stroke updates continuously and never restarts work.
- Edited chunks go first: their own job slots, the front of the workers' queue, the front of
  the upload queue and a 6 ms upload budget. Their rows are scattered on a rayon pool (all
  cores but two), with exactly the same blades in the same order; camera chunks keep one
  thread each. Blades are sorted as 8-byte key/index words (the same order, quicker).
- Chunks near any camera of the last 1.5 s stay: the Scene and Game views share the
  renderer, and side by side each view's frame dropped the other's chunks.
- `RenderStats::busy()`: the Scene and Game views keep redrawing while grass work is pending
  and go idle when it is done.
- Measured with the harness `grass_paint_latency` (200 m terrain, a 64 m square at 1000/m²,
  a 120-frame erase-then-paint stroke across its edge; this machine was busy with other
  builds, so absolute numbers vary run to run):
  - before: painted chunks vanished during the stroke and none came back until it ended;
    after release 3030 frames / 162 s until the grass was back; first load 56.6 s.
  - after: no chunk ever hidden; the chunk under the brush shows each dab in 0.67 s median
    (0.98 s at 90 %) while painting; after release 29 frames / 0.53 s; first load 3.5 s.
  - one full chunk (256k blades): 188 ms on one thread, 61 ms on 8 row threads (busy machine).
- Golden shots unchanged (the scatter gives the same blades in the same order).
- Tests: edits keep the old blades, repeated edits coalesce, edited jobs jump the queue,
  painted grass shows within frames through the renderer (and pending work is reported until
  done), parallel scatter equals serial, `busy()`; timing benches `grass_scatter_cost`,
  `grass_paint_latency` (ignored).

### Done, part 2: tiles under the brush
- A chunk touched by the brush switches to 4 × 4 m tiles, each its own run of blades in the
  pages; a dab re-scatters and uploads only the tiles it touches. The tiles hold exactly the
  chunk's blades (`details::scatter_tile`); while all 16 come up the whole run keeps drawing.
- Tiles run on their own threads from a queue: tiles of a chunk switching to tiles first,
  then the newest dab, nearest the brush first; a tile's waiting job is replaced by a newer
  one, so a stroke never waits on its own stale work.
- Settings changes (sliders dragged in the Inspector) keep every blade drawing and
  re-scatter by distance to the camera, one job per chunk at a time; the brush still goes
  first. A grass species' Healthy↔Dry and Brightness come from a table in the grass uniform
  (`species_mark` in the blade, `grass_dry_mix` / `grass_tint` in the shader): colour edits
  show on the next frame with nothing scattered again.
- Measured (`grass_paint_latency`, CPU at ~74 % from other builds), dab to the blades drawn
  at the brush centre, median / 90 %:
  - brush 0.25 m per frame (about 15 m/s at 60 fps): 76 / 228 ms; before (whole chunks)
    865 / 1082 ms with only half the dabs ever shown during the stroke.
  - brush 1 m per frame (60 m/s): 377 / 1756 ms, mostly chunks switching to tiles; before
    666 / 906 ms with half the dabs shown.
  - grass back after release: 0.3–0.55 s.

### Not done yet
- A fast brush entering an unpainted chunk waits for all 16 of its tiles (one chunk's
  scatter) before its dabs show; switching the chunks around the brush ahead of it would
  hide that.
- The 50 ms goal could not be checked on an idle machine (other agents' builds kept the CPU
  at 55–100 %).
- The editor stroke itself was not scripted end to end (the harness drives the same store and
  renderer calls the Paint Details brush makes).

## Round 30 — Terrain Inspector redesign (branch r30-terrain-ui)

### Done (screenshots `docs/screenshots/r30/*`)
- **Tool bar**: the nine terrain tools as icon buttons in one bar (new icons in the editor's
  icon set: raise / lower, set height, smooth, paint, holes, grass, tree, place, settings),
  with a clear selected state and tooltips (name, what a click and Shift + click do). The
  current tool's name, icon and help sit under it, with World Builder on the right.
- **Brush section**: a shape picker of four thumbnails drawn from the brush's own falloff curve
  (Hard, Firm, Soft, Feather), size (with `[` / `]`), opacity and falloff, each with a reset.
  Set Height adds its target height and a confirmed Flatten All.
- **Palettes of cards** for texture layers, grass and detail types, trees and Place Objects
  prefabs: a thumbnail, the name, a selected outline, a badge (layer coverage %, blades or
  meshes placed, trees planted, objects placed), hover actions (Duplicate, Remove, Select in
  Project when the card stands for an asset), double-click to open the asset, drag a card onto
  another to reorder (layer splat channels, tree instances and the grass's "grow on" layer
  follow), a "+" card with an Add menu and empty states. Cards accept Project assets dropped on
  the palette: textures become layers, models and prefabs (first renderer) become detail
  meshes, `.tree` assets and prefabs are added to their palettes. Cards hold an optional asset
  path, so the coming Grass Type asset can be a card without a layout change.
- **Live previews** (`terrain_previews.rs`): grass tufts are built on the CPU from the
  terrain's blade recipe and the grass shader's arc and colour model (heights, widths, bend,
  seed-head panicles, healthy / dry mix, per-tuft hue and value jitter, dry tips), cached by a
  settings hash and rebuilt the next frame after a change (at most four per frame). Trees use
  their impostor (baked by the tree store's worker) lit on the CPU once per built version.
  Detail meshes and prefabs are rendered offscreen by the engine renderer after the UI pass
  (three per frame, keyed by mesh, material and file versions). Texture layers show their
  albedo with a mean-colour swatch. Placeholders pulse while a preview is made.
- **Settings of the selected item** in sections with icons (Shape, Colour, Density,
  Placement, Rendering, Textures, Tiling, Surface…), aligned labels, units, two-handle range
  sliders (heights, scales, slopes), gradient sliders that show the colour they pick (Healthy ↔
  Dry, Brightness, Dry Tips), colour swatches with hex, a reset button on every changed field
  and a tooltip on every control. The terrain grass card edits its look directly (healthy,
  dry, seed colours, height, width, bend, density) with a link to its rules, wind and patches.
- Fill / Clear with icons, the selected type's name in their tooltips, and a confirmation on
  every destructive whole-terrain action (Clear details, Clear Trees, Clear All objects,
  Fill All Holes, Flatten All).
- **Scene view brush cursor**: coloured by tool (raise blue, Shift lower / erase orange-red,
  paint violet, grass and trees green, smooth cyan, set height yellow), the falloff ring, the
  brush's influence drawn as a dome (height = opacity, profile = falloff curve), Set Height's
  target level and a centre mark.
- New strings in Turkish and Arabic (`[terrain]` tables); cards mirror in right-to-left.
- `MISTY_TERRAIN_TOOL` / `MISTY_TERRAIN_ITEM` capture options open a terrain tool and select a
  palette item.
- Tests: egui harness tests for the tool bar, selecting cards, adding and removing a grass type
  with undo, previews requested and refreshed after a change, reset buttons, confirmed Clear,
  `[` / `]`, and the Inspector's draw time; unit tests for tufts, counts, moving trees.

- **Cost**: the Terrain Inspector on Paint Details with seven grass cards and the large preview
  draws in about 1 ms a frame steady and 1.3 ms a frame while a setting is dragged (a card and
  the large tuft rebuilt every frame), release build, egui harness
  (`terrain_ui_tests::inspector_draw_time_with_previews`); the first frame that shows the
  sections costs 30-40 ms once (icon textures rasterised per size). A tuft costs ~0.1 ms, a
  tree thumbnail ~1 ms once per tree version, an offscreen mesh / prefab thumbnail one small
  renderer pass.
- Screenshots (`docs/screenshots/r30/`): before / after of Paint Details, Paint Trees, texture
  layers and the tool bar; after: detail mesh settings, Place Objects, the brush cursor, Arabic,
  200 % UI scale and the light theme.

- Checks: fmt clean, clippy (workspace, all targets, release) 0 warnings, tests 2223 passed
  with one failure that passes alone (`live_reload::tests::own_saves_and_touches_dont_reload`,
  timing under a fully loaded machine), golden shots 20 / 20 unchanged.

### Notes for the grass painting round
- Any change to a grass species (also look-only ones: Healthy ↔ Dry and Brightness are baked
  into each blade by the scatter) changes `terrain_grass::scatter_key`, which drops every grass
  chunk at once (`keep_chunks(|_| false)`): while a slider is dragged the whole field vanishes
  and re-scatters. Same for the terrain grass's height, width, bend and seed heads (its colours
  are uniforms and update at once). Keeping the old chunks until their replacements are
  uploaded, and moving species dry mix / tint into a small per-species uniform table, would make
  these edits instant in the Scene view.

### Not done yet
- At 200 % UI scale the Inspector panel of a 1920 px window is narrower than its content (all
  components, not only the terrain); the tool bar wraps.
- `scene_files::tests::opening_a_scene_lifts_a_view_left_under_the_terrain` can fail when run
  alongside other tests that open their own project folder (the engine's project root is
  global); it passes alone.
- Brush shapes are falloff presets; custom brush masks (noise, stamps) need brush code
  changes and were left out to keep the merge with the grass painting round easy.
- Grass species colours come from the terrain grass's healthy / dry colours (a species tint
  is not a separate colour yet); a Grass Type asset (`.grass`) is the next round.
## Round 31 — Misty's own names (branch r31-rebrand)

### Done
- The editor uses its own names for what it shows (`crates/misty_editor/src/renames.rs`):
  Inspector → **Properties**, Hierarchy → **Scene Tree**, Project → **Asset Files**, Scene →
  **Editor Scene**, Game → **Scene Viewer**, the GameObject menu → **Object**, the Component
  menu and Add Component → **Archetype** / **Add Archetype** (Remove / Copy / Paste / Move
  Archetype, Revert Archetype Overrides), World Partition → **World Streaming**, Virtual
  Geometry → **Misty Mesh** (menus, Quality, draw modes, Frame Debugger events, profiler rows),
  the asset "Addressable" checkbox → **Load by Address**, the NavMesh Surface's "All Game
  Objects" / "Current Object Hierarchy" → "All Objects" / "Current Object and Children".
  Tooltips, empty states, status text, MCP tool descriptions and tutorials follow ("the Scene
  view" → "the Editor Scene", "the Project window" → "Asset Files", …). Turkish and Arabic
  translations updated (Özellikler / الخصائص, Sahne Ağacı / شجرة المشهد, Varlık Dosyaları /
  ملفات الأصول, Düzenleyici Sahnesi / مشهد المحرر, Sahne Görüntüleyici / عارض المشهد, Arketip /
  نمط, Nesne / كائن, Dünya Akışı / بث العالم) and the glossary in `docs/localization.md`.
- Nothing stored changed: tab ids in saved layouts are the enum variants (a test loads one),
  component names in scenes ("World Partition", "Hierarchy Folder" are shown as World Streaming
  and Scene Tree Folder through `renames::shown_name`, translations keyed by the stored name),
  Lua names (`GetComponent`, `AddComponent`, `CustomInspector`, `HideInInspector`, the
  `GameObject` class and the default "GameObject" object name), MCP tool names
  (`scene.add_component`, …) and tutorial target prefixes (`inspector.…`, `hierarchy.…`).
- Old names keep working: `editor.run_menu_item` maps old paths (`GameObject/3D Object/Cube`,
  `Component/…`, `Hierarchy/<scene>/…`, `Window/General/Inspector` and the other old window
  titles, `… Virtual Geometry`) to the new ones; editor scripts' `MenuItem("GameObject/…")` /
  `("Component/…")` land in the renamed menus; `MISTY_TAB=Game`, `MISTY_OPEN_MENU=GameObject`
  and `MISTY_DRAW_MODE=Virtual Geometry Clusters` still work.
- Tutorials target the new labels (`panel.Scene Tree`, `panel.Properties`, `panel.Editor Scene`,
  `menu.Object/…`); the end-to-end tutorial tests check every target is drawn.
- Guard test `renames::tests::ui_text_and_user_docs_use_mistys_own_names`: tab titles, menu
  commands, tutorials, both translation files, every string literal of the editor's sources and
  the user guides (README, `docs/*.md` except progress / decisions / unity-notes, the release
  READMEs, `docs/api/lua-api.md`) never say Unity, Unreal, Nanite, Lumen or Blueprint.
- User docs describe features on their own terms (no "Unity-like", "Lumen-like", "Nanite-like",
  "in the spirit of Blueprints"); CHANGELOG has an Unreleased entry.
- Screenshots: `docs/screenshots/r31/default_layout_en.jpg`, `default_layout_ar.jpg`.
- Checks: fmt clean, clippy 0 warnings; `cargo test --workspace --release`: editor 816 / 0,
  mcp_e2e 1 / 0, engine 1383 / 1 (`vgeo::auto_picks_one_phase_and_lit_for_a_few_rocks`, a GPU
  counter-readback timing test on an overloaded machine; passes alone), player 11 / 0,
  dedicated server 1 / 0; golden shots 20 / 20. Translation coverage 793/793 in both languages
  (792/792 before), 60/124 component names (same).

### Not done yet
- Kept on purpose: the default name of a new empty object ("GameObject", also the Lua class),
  "Shader Graph", "Animator", "Tile Palette", "Prefab", "Play Mode" (generic words), the
  sample scene name `AddressablesDemo` and sample Lua scripts' comments (generated files).

## Round 32 — User Guide and layout tour (branch r32-guide)

### Done
- **User Guide** in English and Arabic (`docs/guide/en`, `docs/guide/ar`): index, the editor
  layout, finding what is slow (Stats, Profiler, Frame Debugger, Memory Profiler, build report,
  a worked example), quality and rendering (the Editor Scene's Quality menu, Project Settings ▸
  Quality, LODs, occlusion culling, Misty Mesh, Misty GI), Script Graph basics, four
  step-by-step guides (first-person player, door, saving and loading, animation transitions)
  and a glossary. Written with the round 31 names (Scene Tree, Properties, Asset Files, Editor
  Scene, Scene Viewer, Archetypes, Misty GI, Misty Mesh, Script Graphs).
- **Example graphs** for the four guides in `SampleProject/Assets/Guides/` (FirstPerson, Door,
  Saving, Animation), from `sample_guides.rs` with a `matches_its_generator` test and play
  tests: the player walks, looks (yaw and clamped pitch) and jumps; the key door opens with E
  only from its zone and closes again, the auto door opens and closes by itself, both turning
  smoothly; the score and position survive a save, a new prefs load and a fresh scene; the
  Animator goes Idle → Walk → Run → Idle with the speed and plays Jump on its trigger.
- **New Script Graph nodes** (native, typed pins): PlayerPrefs Set/Get Int, Float, String,
  HasKey, DeleteKey, Save; Animator Set/Get Float (with Damp Time), Bool, Integer, Set/Reset
  Trigger; Cursor: Set/Get lockState (graph only).
- **Help ▸ User Guide**: a dockable Guide tab that renders the chapters (headings, lists, code,
  tables, pictures, note boxes, links between chapters) in the editor's language (Arabic right
  to left through `i18n::shape`, Turkish reads English), with search and "Open example" (copies
  the example into the project when missing, then opens its scene and graph). Chapters,
  pictures and example files are built into the binary, so installs need nothing extra. The
  Hub's Learn page has a User Guide card that opens the reader over the Hub.
- **Editor Layout tour** (`assets/tutorials/layout_tour.json`): Scene Tree, Properties, Asset
  Files, Editor Scene, the Quality menu, Scene Viewer, Console, Play, Help. It keeps the open
  scene, brings hidden tabs forward per step, is offered once on the first launch (remembered
  in `hub.json`), and is on Help ▸ Layout Tour and the Hub's Learn page. Its targets are stable
  `tab.<key>` ids (`Tab::key`), so renaming or translating panels doesn't break them. The
  tutorial card's text and buttons are translated now.
- `MISTY_GUIDE=<chapter>` opens the guide on a chapter in capture runs.

### Not done yet
- Screenshots show the pre-round-31 panel names until they are retaken after the rename.
- Menu labels the guide already writes with new names: Assets ▸ Create ▸ Script Graph (now
  "Visual Script Graph") and Window ▸ Script Graphs (now "Visual Scripting").



## Round 26 — NoonValley: a noon meadow valley, and trees at forest scale (branch r26-noon-meadow)

### Done (docs/pending/r26-noon-meadow.md)
- Sample scene **NoonValley** (generator `sample_noon_valley.rs`, `write_noon_valley`,
  `noon_valley_matches_its_generator`, `noon_valley_plays`): a 4 km valley at noon after the
  user's reference photo, ~107 000 placed trees and plants (oak, linden, beech, birch, edge
  trees, bushes, ferns, groundcover), limestone ledges, three hazed ranges, cumulus, a forest
  interior with leaf litter, ferns and broad groundcover. Original procedural 2K textures
  (`tools/noon_valley_textures.py`).
- **GPU-driven tree culling** (`render/trees_cull.rs`): every placed tree in a GPU buffer, one
  compute dispatch per frame for the camera and the cascades (LOD by distance, per-cascade
  LOD by texel), indirect draws; the CPU no longer loops over trees (fast path
  `tree_gpu_cull`). **Hi-Z occlusion** of the camera's lists from last frame's depth while the
  view holds still (`tree_occlusion`). Draw statistics read the lists' counts back.
- **Small plants** (bounding radius under 2 m): LOD and draw distances scale with size; they
  cast only into cascades fine enough for them. **Terrain ▸ Tree Rendering ▸ Shadow Detail**:
  lighter trees and impostors sooner in the cascades (dense forests).
- **Tree ▸ Clumping**: cauliflower crowns of sunlit lumps and dark hollows. Earlier on the
  branch: Tree ▸ Crown Shading, Directional Light ▸ Intensity from Sun Height, Volumetric
  Clouds ▸ Cumulus, Fog ▸ Sky Haze, Contact Shadows ▸ Distant Shadows, grass Depth Shading,
  terrain layer Occlusion.
- **Misty GI** tree proxies cached per terrain in 64 m cells; only cells near the clipmaps are
  gathered each frame (was a loop over every tree, every frame).
- Harness `render/noon_valley_tests.rs`: 9 views, timings, a 250 000-tree stress run, an
  interleaved A/B of the culling modes, sun-height comparisons; parity test
  `tree_gpu_cull_matches_the_cpu_lists`.

### Not done yet
- See the pending doc: GPU time on High at 1080p is above the 12 ms goal (grass and
  screen-space passes dominate the meadow view); look tuning continues on `r26-level`.

## Round 26 (level) — NoonValley scene content against the references (branch r26-level)

### Done (docs/pending/r26-level.md)
- The forested hillside on the right is now a steep wall ~90 m out (foot 90 − 0.2·ahead, a
  600 m hill rising ~40°), closed to the meadow: a 20 m edge band of low crowns and bushes
  (`wood_depth`), a second edge tree per cell, no clearings, litter only behind the edge and
  never on slopes too steep for trees; tree slope limit 47°, limestone only above 48° and not
  on the far ranges (turf there), near-range terraces removed. Terrain height 700 → 1000 m.
- Meadow: Tall Grass 35 → 12/m², greener (dryness 0.35); flowers in small cream and yellow
  drifts (coverage 0.22, patch 12 m, LOD 45); deeper sky tint, higher and taller cumulus.
- Forest interior (treeline wood): half the trees and more canopy gaps, ferns and
  broad-leaved plants filling the light, a quarter birches; eye adaptation down to −4.5 EV
  metered without the brightest fifth; Forest Floor sky light fixed (the field is the light
  that reaches the layer, 1 = open). 2K fern and groundcover atlases. Fern and groundcover
  recipes within the loader's limits (≥ 1 m, ≤ 2.0 crown width), scaled per instance.
- Rounds r9–r14 rendered with the 9-view harness.

### Rounds r15–r20 (scene and engine on this branch, `main` merged in)
- Treeline wood (`wood_cell`): its own trees (Wood Oak, Wood Beech, Sapling: tall bare trunks,
  high thin crowns), slim birches between them, pools of sun where a tree is missing, ferns,
  bushes and carpets of broad-leaved plants; sun Shadow Softness 0.35 (crisp leaf shadows); a
  Fog Volume of light haze among the trunks. About a third of the floor is sunlit, as in the
  reference.
- Light: Misty GI off for this scene (its crown proxies left the open wood's floor with no
  sky light) and ambient light as a gradient (sky, bounce from the sides and below): the
  shade is warm and readable instead of black-blue. Forest Floor sky light 0.5.
- Textures (`tools/noon_valley_textures.py`): new leaf litter (cupped, veined leaves in
  layers, round sticks, baked shade), darker true-green fern and groundcover atlases, a
  cream daisy (the sample daisy tinted); limestone without the brick-like joints.
- Meadow: deeper sky, separate cumulus all over it (engine: Cumulus evens the weather map),
  cream and yellow flower drifts (coverage 0.22, 14 m patches, LOD 80 m), yellower grass,
  hillside crowns darker and lumpier with missing trees as dark hollows, cliff sites where
  the hill crosses the view, far ranges closed woods under bluer haze.
- Grass 800 cards of 4 cm → 520 of 5 cm (the blades are 2.5× MeadowDemo's height): 45 M → 32 M
  triangles in the first view.
- Timings, High 1920 × 1080, r20: first view GPU 18.6 ms / CPU 1.8 ms, inside the wood GPU
  12.2 ms / CPU 1.7 ms (r14: 20.0 / 2.4 and 14.4 / 2.9).
- Shots: `docs/screenshots/r26/level_r20_*.jpg`.

### Not done yet
- First view GPU time is 18.6 ms against the 12 ms goal: grass is ~7 ms of it (visibility
  3.2, shading 2.2, near shadow maps 1.1), trees 3.5. The grass engine is being worked on in
  another branch (`r35-grass-fps`); only scene settings were changed here.
- Against the meadow reference: the limestone cliffs are small pale patches, not large faces
  with trees on top; the cumulus are smaller and more numerous, without grey flat bases; the
  grass blades are coarser; the hill's crowns are more even.
- Against the forest reference: the floor has far less groundcover (bare litter between
  separate ferns), the litter texture is procedural and reads flat close up, trunks have
  stumpy flared feet and no visible branching, sunlit litter is paler.
- Hi-Z tree occlusion stays off (the Low golden drift's cause was not found).

## Round 35 — FPS drops after walking through the grass (branch r35-grass-fps)

Report: playing MeadowDemo is smooth, then after walking forwards through the grass the frame
rate falls from 60 to 20 and stays there until the scene is reopened; looking back does it
too.

### Cause
- A grass chunk kept the finest density level it ever had until it was one and a half draw
  distances away. Walking left a trail of full-density chunks behind the camera: in
  MeadowDemo (800 blades/m², full density up to 6.5 m, drawn to 160 m) a chunk near the camera
  holds 204 800 blades where one 37 m away needs 3 200, and it stayed that big for 240 m.
- Standing still holds 3.0 M blades. The four blade pages (1.86 M blades each) were full
  after 70 m of walking. From 115 m on, new chunks got buffers of their own, which the grass
  visibility buffer can't name, so the lit grass fell back to the old lit pass whenever one
  was in view (also when looking back at the trail). From 150 m on the cull records ran out
  too, and one chunk without records sent every grass pass to the CPU path.
- Measured with the new harness (below): GPU 17 ms → 40 ms ("Main: grass lit" 27–30 ms where
  visibility + shading + composite took 5 ms), 16 M blades and 1.4 GB of grass on the GPU at
  the peak, and it never recovered. v0.3.2 has the same code and does the same.
- Everything else stayed flat over the 2.8 km walk (terrain, shadow maps, Misty GI, clouds,
  TAA, trees, textures 294 MB, CPU render 4 ms): those were measured, not assumed.

### Fix
- **Chunks left behind give their blades back** (`Grass::trim`): a chunk more than one level
  finer than its nearest remembered camera needs keeps the prefix of the level it needs (a
  chunk's blades are in drop-out key order, so a coarser level is a prefix: `level_counts`,
  worked out on the scatter worker). The blades kept are copied on the GPU into a smaller run
  and the old run is freed whole; at most 128 k blades move per frame. Nothing drawn changes
  (a pass draws at most the prefix a chunk's distance keeps).
- **Nothing outside the blade pages.** A chunk the pages have no room for is refused and
  scattered again one level coarser (`Pressure`); the same happens when the blades held pass
  75 % of the pages. The grass goes finer again after ten seconds with room for it, waiting
  longer after each failed try. So a field too dense for the pages gets thinner grass at
  full speed instead of full grass at a third of the speed. `Store::Own` is gone.
- **Pages by kind**: short runs (far chunks) and long runs (chunks near the camera) live in
  pages of their own, best fit, so a short run never lands in the room the next long run
  needs. A page left empty for ten seconds gives its memory back; no grass, no pages.
- **Cull records mirror the pages** (`BladePool::record_at`): one free list instead of two,
  and the records can't run out before the pages.
- Stats: `RenderStats::grass_chunks`, `grass_blades`, `grass_coarser`, and a "Grass: N chunks
  X M blades" line in the Stats overlay (it says so when the grass was thinned for room).

### Numbers
MeadowDemo, High, 1920×1080, headless, one frame per 1/60 s, walking 5 m/s (the machine was
shared with other builds, so absolute times are high; `grass_walk_report`):

| | start | 500 m | 500 m, looking back | 2 km | 2 km, looking back | end (2.8 km) |
|---|---|---|---|---|---|---|
| Before: frame ms (FPS) | 22.3 (45) | 55.7 (18) | 49.0 (20) | 52.0 (19) | 44.8 (22) | 37.6 (27) |
| After: frame ms (FPS) | 18.6 (54) | 20.2 (49) | 18.7 (54) | 19.0 (53) | 19.1 (52) | 19.1 (52) |
| Before: GPU ms | 16.5 | 40.7 | 39.9 | 40.6 | 36.3 | 34.1 |
| After: GPU ms | 15.0 | 15.3 | 14.2 | 15.2 | 14.1 | 14.4 |
| Before: blades held | 2.96 M | 10.2 M | 10.2 M | 10.2 M | 10.2 M | 13.5 M |
| After: blades held | 2.96 M | 3.85 M | 3.85 M | 3.85 M | 3.85 M | 4.21 M |
| Before: grass on the GPU | 354 MB | 953 MB | 953 MB | 967 MB | 967 MB | 1163 MB |
| After: grass on the GPU | 354 MB | 592 MB | 592 MB | 628 MB | 628 MB | 628 MB |
| Before: device allocator | 1441 MB | 2041 MB | 2041 MB | 2055 MB | 2055 MB | 2251 MB |
| After: device allocator | 1441 MB | 1679 MB | 1679 MB | 1715 MB | 1715 MB | 1715 MB |
| Before: process private | 2370 MB | 2893 MB | 2895 MB | 3138 MB | 3138 MB | 3138 MB |
| After: process private | 2348 MB | 2614 MB | 2614 MB | 2610 MB | 2610 MB | 2609 MB |

v0.3.2 (tag, same harness, the walk at 0.4 scale: 200 m out, 800 m at the "2 km" mark):
GPU 15.1 ms at the start, 41.3 ms after 200 m, 40.7 ms after 800 m, 39.9 ms at the end
(1.1 km); first chunk in a buffer of its own at 115 m, first without cull records at 150 m,
as on main; 21.7 M blades held at the end with 127 chunks outside the pages.

While walking (median per second): GPU 15.5 ms in every stretch after the fix (before: 17.0
for the first 100 m, then 38–41); the peak is 4.6 M blades and 628 MB (before 16.0 M and
1382 MB). The visibility buffer and GPU culling stay on throughout, nothing is refused or
thinned.

### Tests
- `grass_walk_tests.rs`: the harness (`grass_walk_report`, ignored: minutes long) and its
  script (`poses_follow_the_script`).
- `grass_trim_tests.rs`: coarser levels are prefixes of a chunk; the trim rule; a moving
  camera's trail counts as a few places; pressure rises at once and falls slowly; a long walk
  over a synthetic meadow holds no more than 1.5 × standing still, every chunk at least as
  fine as a fresh look needs; trimmed chunks hold exactly their first blades and cull records
  on the GPU (read back); a field too dense for its budget goes coarser and recovers; full
  pages refuse a chunk and the grass goes coarser with every chunk still culled; a 400 m walk
  through MeadowDemo itself stays under 1.5 × standing still with nothing refused.
- `grass_pool.rs`, `grass_cull.rs`: best fit, pages by kind, trimming moves a run out of the
  way, full pages refuse, an empty page gives its memory back, records mirror the pages, the
  arena grows to cover and moves runs.

### Not done
- The pages still grow to four (456 MB of pages for 4 M blades): two kinds of page, each a
  little over one page's worth. Bounded, but there is room to pack tighter.
- Each finer level of a chunk is still scattered and uploaded whole; holding a chunk as one
  run per level would upload each blade once and make trimming a plain free.
- The editor Console keeps every message and lays out every row each frame
  (`panels/console.rs`): a script that prints every frame slows the editor the longer it
  plays. Not what this report was about (MeadowDemo's scripts don't print), not fixed here.
- NoonValley (branches r26-*) has the same grass numbers (800 blades/m², 160 m, 5.8–6.5 m)
  plus a species and more flowers: same fault before, same fix; not measured there.
## Round 33 — Engine-wide QA (branch r33-qa)

Review of terrain, scenes/prefabs/assets, Lua and Script Graphs, tilemaps/2D, physics, audio,
MCP and the renderer; the bug list, routing and status per bug are in
`docs/pending/r33-qa.md`. Each fix comes with a regression test.

- Crashes: Script Graph constants or pass-through subgraphs wired in a loop (stack overflow
  while wiring); Lua tables that hold themselves passed to a graph or returned from `OnSave`
  (overflow / exponential save); Lua handles kept after `Destroy` (Rigidbody, Rigidbody2D,
  Collider2D, Tilemap, Terrain, Network, Ragdoll) raise "object was destroyed" instead of
  aborting; a deleted mixer snapshot; zero world scale (NaN physics poses, 3D and 2D); a NaN
  point in a Polygon Collider 2D; corrupt pak indexes; terrain blade counts on huge texels;
  terrain undo after an outside resize; garbage terrain resolutions with holes; MCP clients
  sending endless lines.
- Data loss: asset saves (scenes, prefabs, terrains, materials, meta files) are atomic (temp
  file + rename); renaming the prefab open in Prefab Mode no longer recreates the old file;
  prefabs with components this version doesn't know are not written back (the components
  would be deleted); a rename whose meta can't follow is rolled back; terrain Inspector edits
  in Play mode are reverted on Stop like the scene instead of being saved without an undo step.
- Wrong behaviour: tile seams with MSAA in the 2D Platformer template (unlit draws sample at
  the centroid of the covered samples); renames with a dot in the name; World Partition tiles
  got the whole flower mask; terrain undo memory ignored maps kept whole.
- Flaky tests: frame-count waits replaced by clock deadlines (package registry, meadow
  project creation); the terrain view test retries only when another test moved the project
  root (`assets::root_generation`); MCP info file removal retried; shader graph demo render
  reloads its shaders and uses absolute texture paths.

## Round 34 — Weather: rain, snow, storms (branch r34-weather)

### Done
- **Weather component** (GameObject ▸ Rendering ▸ Weather; `misty_engine::weather`): a weather
  volume, **Global** (everywhere: the precipitation is spawned round the camera) or an **Area**
  box / sphere with a blend distance (a rainy valley, a storm cell). Volumes blend by priority
  and weight like Post-process Volumes; an implicit clear layer lies under them all.
- **Every value is free** (`WeatherParams`): rain intensity, drops/m³, drop size, fall speed,
  streak length, opacity, refraction; snow intensity, flakes/m³, flake size, fall speed,
  swirl, opacity; wind direction, speed, turbulence, gusts; splashes, mist, visibility;
  lightning rate, distances, brightness; cloud cover and density, sun and sky dimming.
  13 presets fill them: Clear, Drizzle, Light Rain, Rain, Heavy Rain, Downpour, Thunderstorm,
  Sleet, Flurries, Light Snow, Snow, Heavy Snow, Blizzard.
- **Timeline** per volume: Sequence (looping or not) or weighted Random states with min / max
  holds and a blend time each; every value interpolates (wind along the short arc, visibility
  by extinction, rain → snow passes through sleet). "Demo Cycle" = clear → light rain →
  thunderstorm → snow → clear.
- **Runtime** (`Scene.weather`, not saved): runs in Play and in Edit mode while a volume has
  Preview in Edit Mode on. Wetness, puddles and snow cover build up and dry / melt per volume
  (speeds in seconds; rain melts snow faster; meltwater keeps the ground wet).
- **Renderer** (`render::weather`):
  - A top-down **occlusion map** of the shadow casters within 48 m (after the shadow maps): rain
    and snow stop at roofs and tree crowns, splashes land on roofs, floors under cover stay dry.
  - **Rain**: procedural GPU streaks in two boxes round the camera (anchored to the world,
    moved by the accumulated wind drift and fall; no CPU work per drop), refracting the opaque
    scene copy, with a Fresnel sky rim, sun forward scattering and point / spot light glints
    (rain shines in street lamps at night); thin far drops fade instead of aliasing; distant
    rain sheets so density reads far away; curtains under far area volumes.
  - **Snow**: flakes of three shapes (soft, star, clump) that tumble, swirl and drift; big soft
    flakes near the lens (depth of field); streaks in blizzard wind.
  - **Splashes** spawned on the GPU from the depth buffer where it rains, the surface faces up
    and is open to the sky (crown droplets and a ring), plus ground mist in heavy rain.
  - **Surfaces** (`weather_surface.wgsl` in `shade`, foliage in `shade_grass` / `shade_tree`):
    porous materials darken, everything gets glossy, puddles on flat open ground with raindrop
    ripples, snow on upward faces (patchy while thin, optional snow line), nothing under cover.
  - **Lightning**: Poisson strikes with 1–4 return strokes; the flash lights every surface open
    to the sky and the clouds; branching bolts drawn into the HDR and glow images (they
    bloom); thunder after distance / 343 m/s.
  - **Haze** lowers visibility (and thickens volumetric fog); **Drive Clouds** moves the
    volumetric clouds towards the weather's cover (turning overcast clouds on) and dims the
    sun / moon and sky light; optional **lens drops**.
  - Quality ▸ Weather (Off / Low / Medium / High) scales particles, splash slots and the
    occlusion map; Off keeps wet / snowy surfaces and lightning.
- **Audio**: rain loops (light and heavy, crossfaded by intensity), a wind loop and near /
  far thunder, generated once into `Library/Weather/` when the volume has no clips of its own;
  they play through the volume's Audio Mixer group.
- **Lua `Weather`**: SetState(name, seconds), ClearState, SetRain / SetSnow / SetIntensity /
  SetWind / SetLightning, Strike, GetState / GetRain / GetSnow / GetWind / GetVisibility /
  GetWetness / GetPuddles / GetSnowCover, SetWetness / SetSnowCover, IsScripted, `states`.
  Events OnLightning(distance, position, intensity), OnThunder(distance, volume),
  OnWeatherChanged(state, seconds) reach Lua scripts and Script Graphs (Custom Event nodes) as
  broadcast custom events; the API table lists the members as graph nodes.
- **Editor**: Inspector with Preview in Edit Mode, a live status line, Coverage, the preset
  dropdown and sections (Rain, Snow, Wind, Ground, Lightning with Strike Now, Sky, Surfaces,
  Effects, Timeline, Audio); Turkish and Arabic strings.
- **WeatherDemo** sample (`sample_weather.rs`): asphalt square, shelter, trees, stone wall,
  street lamp, the demo cycle; keys 1–8 switch states, L strikes, 0 returns to the timeline.
- **GPU cost** (WeatherDemo, 1920×1080, Quality High, `weather_gpu_timings`; the machine was
  busy with other agents' builds):

  | Case | Frame | Weather composite | Occlusion map | Main pass |
  |---|---|---|---|---|
  | No weather | 3.11 ms | — | — | 0.82 ms |
  | Heavy Rain + splashes + wetness | 3.86 ms (+0.75) | 0.91 ms | 0.007 ms | 1.09 ms |
  | Downpour | 4.42 ms (+1.31) | 1.48 ms | 0.007 ms | 1.09 ms |
  | Thunderstorm | 4.06 ms (+0.95) | 1.11 ms | 0.007 ms | 1.09 ms |
  | Blizzard + snow cover | 3.86 ms (+0.75) | 0.94 ms | 0.007 ms | 1.07 ms |

  The main pass grows ~0.27 ms with wet / snowy surfaces (the scene copy for refraction is in
  it too). Target was under ~2 ms for the heavy cases: all are.
- Screenshots (`weather_screenshots`, 1600×900 JPEG): `docs/screenshots/r34/light_rain.jpg`,
  `downpour_mist_splashes.jpg`, `thunderstorm_night.jpg`, `wet_street_puddles.jpg`,
  `snow_falling.jpg`, `blizzard.jpg`, `snow_covered.jpg`.
- Tests: presets, transitions (endpoints exact, rain → snow through sleet, visibility by
  extinction), the timeline state machine (sequence, looping / not, weighted random without
  repeats), area volume blending by priority, wetness / puddle / snow accumulation and decay,
  thunder delay from distance, lightning rate and bolts, the script layer, wind gusts, the Lua
  API with events, generated audio, shader validation (naga) of every lit shader with the
  hook, GPU: rain and snow draw, a disabled volume changes no pixel, no snow under a roof
  (and snow there without the occlusion map); WeatherDemo matches its generator and plays.

## Integration 1 — rounds 33, 27 and 34 together (branch integrate-1)

### Done
- Merged onto main d1703eb3, in this order: `r33-qa` (engine-wide fixes), `r27-moon` (moon,
  moonlight, stars, Milky Way, aurora, meteors), `r34-weather` (rain, snow, storms, timeline).
  Conflicts: this file (every section kept), the Turkish and Arabic files (the terrain / guide
  tables and the weather table both kept) and the view bind group in
  `render/volumetrics/mod.rs`: it now takes the sky's five entries (the moon texture is binding
  11) and the weather's two (bindings 24 and 25).
- Round 27 loose ends: two clippy warnings; the night sky GPU cost table above; the aurora
  screenshots re-rendered after the white-noise step jitter (no diagonal hatching; a single
  frame without TAA shows fine grain instead). MoonDemo, NightSkyDemo and WeatherDemo were
  rewritten by their write tests on the combined engine: byte-identical, nothing to commit.
- Round 34 loose ends: `api_docs::SOURCES` did not read `scripting/weather_api.rs`
  (`every_binding_source_is_read` failed); docs/api regenerated (900 members, 107 sections).
- New names and languages: the Lighting window's Moon and Night Sky sections go through
  `tr!()` with Turkish and Arabic entries (`[night_sky]` tables; "Manual", "Phase", "Time",
  "Core" and "Pulse" carry a `#context` so they do not take over other meanings); the Weather
  component's Preview tooltip says Editor Scene and Scene Viewer.
- `MISTY_LIGHTING_SCROLL=<px>` scrolls the Lighting window and opens the Night Sky groups
  (screenshots).
- Checks on the combined branch: `cargo fmt --all --check` clean; clippy `--workspace
  --all-targets --release` 0 warnings; `cargo test --workspace --release --no-fail-fast`:
  editor 868 passed, engine 1466 passed, player 11 passed, 0 failed; golden shots 20/20 with
  the same numbers as main (mean 0.00–0.39, changed at most 0.03 %): the round 33 shader change
  (`uv_centroid`, used only by `fs_unlit`) moves no golden shot.
- Screenshots of the editor on the combined branch: `docs/screenshots/integrate-1/`.

### Not done yet
- The Lighting window outside the Moon and Night Sky sections is still English only.
- Weather and the night sky were not tuned together (a storm at night dims the moonlight
  through the same sun dimming; no look pass was done on it).

## Round 39 — The Console no longer slows the editor (branch r39-console)

A script that printed every frame made the editor slower the longer Play ran: the Console kept
every message, and each frame it went through all of them and laid out every row (with Collapse
on, it compared every message with every row before it).

### Fix
- **A bounded store** (`console.rs`): the newest 10,000 lines in a ring; older ones are dropped.
  Edit ▸ Preferences ▸ **Console Lines** picks 1,000 to 100,000 (saved per user in `hub.json`,
  Turkish and Arabic labels).
- **Counters count everything** logged since the last Clear, dropped lines included (the ⛔ ⚠ ℹ
  buttons, Error Pause, `editor.get_state`); they are kept as numbers, not counted each frame.
- **Rows are kept, not rebuilt**: the filtered (and collapsed) rows live with the store and
  follow it line by line as lines are logged and dropped. They are built again only when a
  level toggle, the search or Collapse changes. Collapse still merges every identical message
  (same level and text) into one row with a count, now through a hash table.
- **Only the rows in view are drawn** (`ScrollArea::show_rows`, rows of one fixed height).
- **Indices never change** (MCP `editor.read_console`): a line keeps its index when older lines
  are dropped and across Clear. A client that asks `since` an index that is gone gets what is
  left, with `dropped` (how many lines it missed) and `first` (the oldest index kept).
  `counts.total` is now everything logged since Clear. `docs/mcp.md` has the details.
- The selection is a line index: it stays on its line as others come and go, and ends when the
  line is dropped.

### Numbers
`console_flood_report` (release, 900×400 panel, the egui test harness; "no a11y" is the same
frame without the harness's accessibility tree). Lines of about 45 characters.

| Lines logged | Before: kept | Before: ms a frame (no a11y) | After: kept | After: ms a frame (no a11y) |
|---|---|---|---|---|
| 1,000 | 1,000 (0.15 MB) | 5.1 (2.9) | 1,000 (0.15 MB) | 0.19 (0.15) |
| 10,000 | 10,000 (1.8 MB) | 118 (61) | 10,000 (1.8 MB) | 0.17 (0.13) |
| 50,000 | 50,000 (8.1 MB) | 430 (244) | 10,000 (1.8 MB) | 0.15 (0.12) |
| 200,000 | 200,000 (32.3 MB) | 2,466 (1,205) | 10,000 (1.8 MB) | 0.17 (0.12) |

- Collapse on, 10,000 different messages (no a11y): 299 ms a frame before, 0.17 ms after.
- A script printing two lines every frame: before, a frame cost 2.7 ms around frame 150 and
  91 ms around frame 2,950 (and kept growing); after, 0.28 ms and 0.16 ms, and 0.28 ms around
  frame 29,950 (0.21 ms with Collapse on), the store steady at 1.5 MB.

### Tests
- `console_tests.rs`: memory bounded after 200,000 lines; counters after dropped lines and
  after Clear; indices across dropped lines; level toggles and search; Collapse; the rows are
  not rebuilt while lines stream in (either mode); 20,000 random steps (log, toggle, search,
  Collapse, Clear, resize) where the kept rows equal rows worked out from scratch.
- `panels/console_tests.rs` (egui harness): as many rows drawn of 60,000 lines as of 500; a
  frame after 200,000 lines costs no more than after 1,000; 15,000 frames of a printing script
  do not slow down; counters, search, toggles, Collapse and Clear after lines were dropped;
  click selects, the detail pane shows the whole message, the selection ends with its line.
- `mcp/tests.rs`: `since` across dropped lines and Clear, paging, counts, menu commands.

### Not done
- A single huge message is kept whole: the store bounds the number of lines, not their bytes.
- The Console page of the User Guide does not mention the limit yet.

## Round 42 — Material Properties: square texture slots and one aligned list (branch r42-material-ui)

The Standard material's Properties view was a list of texture dropdowns with the colours and
sliders squeezed in beside them, and five feature sections that each started with their own
checkbox. It is now one aligned list: names on the left, values in one column on the right, and
a small square texture slot in front of every map's name.

### The texture slot (`panels/inspector/texture_square.rs`)
- An 18 px square showing the texture's thumbnail (dark when empty, `?` when the file is gone).
- **Drop** a texture from Asset Files on the square or on the name beside it. While a texture
  is dragged every slot gets a thin outline; the one under the pointer a bright one.
- **Click** opens a picker: a search field (focused, Enter takes the first match), "None", and
  the project's textures with thumbnails. Only the rows in view are drawn.
- **Right-click** clears the slot, and so does the small **×** shown in the top-right corner
  of a filled slot under the pointer (8 px, so a click in the middle still opens the picker).
  **Ctrl+click** or a **double-click** shows the texture in
  Asset Files; the Properties view stays on the material.
- **Hover** shows a 96 px preview with the texture's name and path.
- A **normal-map slot** whose texture is not imported as a normal map gets a row under it:
  "Fix Now" and "⚠ Not imported as a normal map". Fix Now sets the texture's import type to
  Normal Map (as Apply in the texture's own Properties does).
- Thumbnails come from the existing thumbnail store. The import type is read once per texture
  and again after import settings are applied, so nothing is read from disk while drawing.
- `.shader` and Shader Graph materials use the same slot for their texture properties.
  The other panels (terrain layers, particles, UI images…) keep the dropdown for now.

### The layout (`material.rs`, `material_features.rs`)
| Section | Rows |
|---|---|
| (top) | Shader, Rendering Mode |
| Main Maps | Albedo (slot + colour), Alpha Cutoff (Cutout only), Metallic (slot + slider; a map replaces the slider), Smoothness, Source, Normal Map (slot + strength when set), Height Map (slot + depth slider when set, then Steps), Occlusion (slot + strength when set), Detail Mask, Emission (checkbox; on: Color slot + colour, Intensity), Tiling, Offset, Rotation |
| Secondary Maps | Detail Albedo x2, Normal Map (+ strength when set), Tiling, Offset |
| Advanced Surface | Clear Coat, Translucency, Anisotropy: each a checkbox that opens its rows |
| Rendering Options | Specular, Specular Tint |
| Advanced Options | Render Queue (From Shader + number, read only), Double Sided |

- The Height Map and the detail maps have no checkbox any more: assigning a map turns the
  feature on, clearing it turns it off. A file with maps but the feature off (from the old
  checkbox) shows an "Enabled" row.
- Sliders and their number fields fill the value column, so they all line up; lone numbers and
  the X / Y pairs fill it too.
- The Unlit shaders use the same rows (slot, Tiling, Offset, Advanced Options).
- Every change is still one undo step, and the preview (sphere, cube, plane) is unchanged.
- New texts are in Turkish and Arabic (`[material]` in `locales/tr.toml` and `ar.toml`).

### Not in Misty's material, so not in the list
- UV Set, Specular Highlights / Reflections switches, GPU instancing and a per-material render
  queue override. The Render Queue row shows the shader's queue, read only.
- Editing several materials at once was not possible before and still is not.

### Tests
`panels/inspector/material_tests.rs` (egui harness): a texture dropped on a slot or on its name
is assigned and other assets are ignored; right-click clears and undo brings the texture back; the × shows on hover only and clears;
the picker searches, assigns and clears; Ctrl+click shows the texture in Asset Files; Alpha
Cutoff only for Cutout; Emission opens its rows; strength rows only with a map; a detail map
turns the detail maps on; the normal-map warning and Fix Now; Advanced Surface checkboxes.

### Screenshots
`docs/screenshots/r42/` (the Properties view cut out of the editor window; a temp copy of the
sample project):
- `compare_demo.png`, `compare_demo_scrolled.png`, `compare_bumpy.png`: before on the left,
  after on the right, for a material with most features on and for a plain one.
- `before_*.png` / `after_*.png`: the halves on their own; `after_window.png`: the whole editor.
- `after_picker.png`: the texture picker (`MISTY_TEXTURE_PICKER=Albedo` keeps that slot's
  picker open for a capture).
- `after_demo_tr.png`, `after_demo_ar.png`: Turkish and Arabic (right to left).

### Notes
- Section headers are as tall as the other rows. With taller headers the rows under a new row
  (Alpha Cutoff appearing, say) took several frames to settle: the grid lays a row out against
  the height the row at that index had the frame before.
- The sample project's `Bumps.png` is not imported as a normal map, so materials that use it
  show the warning until Fix Now is pressed.
## Round 41 — A texture dropped on an object becomes its material (branch r41-texture-drop)

Drag a texture from Asset Files onto an object, in the Editor Scene or on its Scene Tree row:
the editor makes a material for it in `Assets/Materials` and assigns it. The rule is written at
the top of `crates/misty_editor/src/texture_drop.rs`.

### What happens
- **Colour texture**: `Assets/Materials/<TextureName>.mat` (the folder is made when missing)
  with the default lit settings (the project's default Material preset when there is one) and
  the texture as its albedo map, assigned to element 0 of the object's Materials.
- **Reuse**: a material of that name whose albedo map is that texture is assigned instead of
  making another. Materials are read through their GUID table, so the comparison follows a
  texture that was moved or renamed. A different material of that name is left alone and
  `<TextureName> 1.mat` (then 2, 3…) is tried the same way.
- **Normal map**: Texture Type: Normal Map in the import settings, or a name ending in
  `_Normal`, `_NormalMap`, `_Normals`, `_Nrm`, `_Nor` or `_N` (any case; after `_`, `-`, `.`
  or a space). It goes into the Normal Map slot of the material the object has now, edited in
  place when that material is a writable asset under `Assets/` (every object using it shows
  it). On Default-Material, a read-only package material or a missing one, a copy with the
  normal map is made in `Assets/Materials` and assigned (`Rock_Normal.png` on Default-Material
  gives `Rock.mat`).
- **Renderers**: Mesh Renderer and Skinned Mesh Renderer take the material. A Sprite Renderer
  (no mesh renderer on the object) takes the texture as its sprite and no material is made.
  An object without any of them gets a toast and nothing changes.
- **Element**: always element 0. Extra Materials elements draw the whole mesh again and the
  Editor Scene picks by bounds, so there is no element under the cursor to find.
- **Undo**: one step ("Assign Material", "Assign Normal Map" or "Assign Sprite"). The material
  file that was made stays, as with Assets ▸ Create.
- **Files**: the `.mat` is written whole (the atomic write of round 33) with the GUIDs of its
  textures; its `.meta` comes from the same asset scan every new asset goes through.
- **Feedback**: a toast in the status bar ("Created material Rock.mat", "Assigned material
  Rock.mat", "Set the normal map of Rock.mat") and a Console line; Asset Files keeps the
  folder it shows, so several textures can be dragged from it one after another (a made
  material is selected only when Asset Files already shows `Assets/Materials`); the scene
  selection and Properties stay as they were. While a material or
  texture is dragged, the Scene Tree row under the cursor is outlined and the Editor Scene
  shows the name of the object under the cursor (materials had no such feedback before).
- **Prefab instances**: the new material is an override of the instance (Mesh Renderer ▸
  material), like any other change to it.
- New texts are in Turkish and Arabic; the User Guide (Layout ▸ Asset Files) has a line on it
  in English and Arabic.

### Tests
`texture_drop/tests.rs`, each in its own temp project: a drop on a cube in the Editor Scene
(egui harness: the file, the assignment, the toast, one undo step, redo; a drop on empty space
does nothing; Asset Files keeps its folder and selection); a drop on a Scene Tree row (egui
harness); two drops in a row keep Asset Files where it was, and a material made while Asset
Files shows `Assets/Materials` is selected there; the same texture again reuses the
material; another material of that name gives `Rock 1.mat`, which is reused afterwards; a
same-named texture from another folder gets its own; normal maps by name and by import
settings; a normal map into the object's material with undo and redo; a normal map on
Default-Material; no renderer; a Sprite Renderer; a prefab instance override; the `.mat` holds
the texture's GUID and gets a `.meta`.

### Not done
- Asset Files drags one asset at a time, so "several textures at once" cannot happen yet.
- A normal map dropped on an object edits the shared material asset; there is no question
  asked first.
- Metallic, occlusion and emission maps are not detected by name; they become albedo
  materials.
## Round 38 — Weather polish: rain that reads as water (branch r38-weather-polish)

### Done (docs/pending/r38-weather-polish.md)
- **Properties bug** (commit "Weather Properties: edits always show"): with a timeline on, the
  runtime took its values from the timeline's states and ignored the volume's own, and a
  script's `Weather.SetState` (WeatherDemo's keys 1–8) held the weather until
  `Weather.ClearState`: either way presets and sliders did nothing. Now picking a preset or
  changing any weather value turns the timeline off (its states are kept) and clears the
  script's hold, so the edit shows on the next frame in the Edit Mode preview and in Play. A
  notice names what is driving the weather ("Timeline is driving the weather (now: …)" with
  Turn Timeline Off, "Script override: Blizzard" with Clear Override). Turkish and Arabic
  strings. A timeline's Time Scale left out of a scene file loads as 1 (it loaded as 0).
- **Rain drops are lenses, not white lines** (`precipitation.wgsl`, `vs_rain` / `fs_rain`):
  - A drop shows the scene behind it flipped across its width, read from a small blurred
    chain of the opaque scene (`render/weather/scene_blur.rs`: 6 levels from half size,
    recorded only while it rains; cheaper to sample than the full-size copy read before).
  - **Depth**: near drops are drawn larger; about half of the drops within a few metres are
    out of focus (wide, soft, see-through), the rest sharp; further out they are thin beads;
    far away they merge into soft streaky sheets with drifting bands.
  - **Bead shape and motion** (what the user liked in Sleet): a shorter exposure
    (`SHUTTER` 1/55 s at Streak Length 1), a round head thinning up its tail, every drop
    swaying on its own small loops and carried differently by the wind, so no two lie along
    one line; size, speed (big drops fall faster), brightness and beading differ per drop.
  - **Light**: a thin drop carries the light around it (the widest blur level, seen through
    the same haze as the pixel behind it), so it vanishes against the sky and shows against
    trees; every drop has a highlight of the sky so rain shows against dark ground; glints
    from the sun (towards it: the scattering direction was reversed in r34, which lit drops
    looking away from the sun), lamps and lightning. A flash lasts a millisecond: it lights
    the head of each streak only (frozen drops).
  - Heavy rain pulls distant colour towards grey; an early frustum test skips the occlusion
    lookups of drops outside the picture.
  - **Streaks bend**: a drop's path through one exposure bows or snakes a little (a few % of
    its length, more in turbulent wind, less for big drops and out-of-focus ones), drawn in
    the fragment shader inside a widened quad, so no streak is a ruled line.
  - **Against a bright sky** a drop is a little darker than the sky (a lens shows the ground
    below the horizon), so rain reads as grey streaks there instead of vanishing.
- **Rain stops in terrain tree crowns**: the occlusion map also draws each terrain tree's
  crown near the camera (the upper half of the ellipsoid the GI uses, at 0.8 of its size,
  `occlusion_crowns.wgsl`), so it no longer rains through the oaks and the ground under
  them stays drier.
- **Presets**: Sleet is thicker (22 drops and 18 flakes per m³), Thunderstorm by day lets
  more light through (sun dimming 0.78, ambient 0.48; nights stay dark).
- **Night storms are dark**: the test's night is a real one (sun under the horizon, r27's
  moon and night sky); rain is lit by lamps and lightning. The flash lights the cloud base
  in cloud shapes round the strike instead of whitening the whole sky and the haze.
- **Splashes**: droplets on real arcs (2–5 per splash), a thin broken ring, a low smooth
  corona from the biggest drops only (tall ragged cups read as grass tufts); clear water lit
  by the light around it; every splash differs.
- **Snow**: the same depth of field as the rain (big soft flakes near the lens), flakes
  smeared by wind are soft dashes instead of pills, a faster exposure, far flakes thin into
  the haze in a gale.
- **Wet surfaces**: the water film is uneven (patches of gloss, not one mirror); soil and
  turf (very rough surfaces) darken but stay dull and hold fewer puddles; puddles have soft
  edges; the noise behind puddles and snow patches no longer shows square cells or facets.
- **Lens drops** (still optional, off by default): a few lumpy drops at a time, each a small
  upside-down, out-of-focus view.
- **WeatherDemo** is a lane by an oak wood: a small terrain (`Terrains/WeatherDemo.terrain`)
  with grass and a gravel path, terrain trees from `Assets/Trees` (oak, young oak, birch),
  rock prefabs, a timber shelter with a pitched roof and a bench, two street lamps. It
  starts in manual (Rain; the demo cycle's states are kept with the timeline off). Keys as
  before, plus N for night / day.
- `weather_screenshots` takes `MISTY_WEATHER_OUT` / `MISTY_WEATHER_SHOTS`, `weather_demo`
  takes `MISTY_WEATHER_SCENE`; tests can read the composite shaders from disk
  (`MISTY_WEATHER_SHADERS`) to tune the look without rebuilding.
- **GPU cost** (`weather_gpu_timings`, weather composite pass, r34's WeatherDemo view so the
  numbers compare; the machine was busy with other builds, so ±0.1 ms is noise):

  | Weather | r34 (2026-11-11) | r38 first agent's shaders | r38 final (two runs) |
  |---|---|---|---|
  | Heavy Rain | 1.137 ms | 1.250 ms | 1.252 / 1.313 ms |
  | Downpour | 1.575 ms | 1.854 ms | 1.880 / 1.948 ms |
  | Thunderstorm | 1.250 ms | 1.466 ms | 1.530 / 1.476 ms |
  | Blizzard + snow cover | 1.215 ms | 0.772 ms | 0.776 / 0.914 ms |

  Rain costs 0.1–0.35 ms more than r34 (the lens look: blurred-scene reads, depth of field
  on the near drops, bent streaks); snow is cheaper. The bend is kept narrow on wide blurred
  drops and the blur disc is capped at 5 % of the picture height to hold it there. The r34
  binary could not be re-run the same day (its copy was on the failing D: drive), so the
  r34 column is the day before's measurement. On the new WeatherDemo view (terrain, oak
  wood) the occlusion map with crowns costs 0.061 ms.
- **Screenshots** (`docs/screenshots/r38/`): `before_light_rain.jpg` and
  `before_downpour_mist_splashes.jpg` (r34) against `light_rain.jpg` and `downpour.jpg`;
  `rain_near_drops.jpg`, `rain_against_sky.jpg`, `rain_looking_down.jpg`,
  `thunderstorm_looking_down.jpg`, `thunderstorm_night.jpg`, `thunderstorm_night_flash.jpg`,
  `thunderstorm_day.jpg`, `rain_night_lamp.jpg`, `rain_lamp_day.jpg`, `sleet.jpg`,
  `wet_lane_puddles.jpg`, `rain_lens_drops.jpg`, `rain.jpg`, `clear.jpg`, snow:
  `snow_falling.jpg`, `blizzard.jpg`, `snow_covered.jpg`, `snow_night_lamp.jpg`.
- **Not real yet**: near drops are still billboards (no true refraction of the 3D scene, only
  the flipped blurred screen); puddle reflections are softened, not fixed (the hard edges
  come from the screen-space reflection / probes); tree crowns block rain as solid
  ellipsoids (no drip, no thinning); no Inspector values for the bead shape or blur.

## Round 43 — Bug hunt 2: security, crashes on bad input, long-session growth (branch r43-qa2)

QA round 2: the open items of round 33 and a first pass over areas round 33 barely covered
(networking, player builds, saves, addressables, localization, UI, animation import and
retargeting, audio streaming, Script Graphs, the Lua sandbox, malformed model files,
long-session growth). One commit per bug, each with a regression test; the running list with a
status per bug is `docs/pending/r43-qa2.md`.

### Security
- **Lua sandbox**: game scripts had `io`, `os` and `package` (`os.execute`, `io.open`,
  `require`, `dofile`), also while the Inspector read a script's fields. They now get
  `table`, `string`, `math`, `utf8`, `coroutine` and `os.time/clock/date/difftime`; `load`
  takes source text only. Editor scripts keep the full libraries.
- **Paths from scripts, saves and the network** must be project-relative (`vfs::project_file`):
  no absolute, drive, UNC or `..` paths for Instantiate, a remote spawn's prefab, a server's
  scene, audio clips, input action assets, Script Graphs and saved prefabs.
- **Network fragments**: bounded reassembly (a peer could make the other side allocate GBs);
  spawn messages with NaN/infinite poses are dropped.

### Crashes and hangs
- Endless loops in game Lua stop after 5 s per frame with an error (the editor froze).
- glTF: short animation outputs, node cycles and huge key times (also FBX and retargeting);
  appending curve keys is O(1).
- Streamed looping clips without frames, bundle dependency cycles, nested plural strings,
  dropdowns whose item is the template, 0×0 images, gzip save bombs, tilemap drags far
  outside tiny grids, sprite grid slices with absurd padding, `.vgraph` files with huge or
  repeated node ids.

### Wrong behaviour and growth
- `Scene::save` writes only the active scene when several are loaded; Save As / Open accept
  project roots spelled differently; tile sprite colliders follow rotation and flips; MCP
  enforces the schema limits it advertises; `StartClient(addr, port)` replaces a port already
  in the address; `WithTargetBinding` checks its binding; NaN field of view is refused.
- UI / Timeline / Video / PlayerInput listeners of destroyed objects are dropped; respawned
  local players no longer add an action set each time; a Script Graph node logs at most four
  different errors.

### Not done
B3 (internal tile edges), B6 (chunk cache key, perf), T6 (terrain hole indices, perf),
`asset.read_text` reading whole files, and the X items listed in the pending notes. Physics
exit events for destroyed objects stay as they are (as in the familiar engine). F10 belongs
to `r40-freeze`.
## Round 44 — First open of a project and shader compiles off the main thread (branch r44-first-open)

The editor went "Not Responding" for seconds when a project opened for the first time: every
base pipeline compiled before the first frame, then the scene's first render compiled its
material variants and decoded its textures in one frame. Now the window draws from about
1.2 s after start and loading happens behind it.

- **Renderer made on a worker** (`viewport::GpuStart`): `Renderer::with_queue`, the quality
  level and one throwaway warm-up frame (`Renderer::warm_up`: a cube on a plane, a shadowed
  sun, AO, contact shadows, SSGI, SSR and Misty GI on, so their passes and compute pipelines
  are made) run on `misty background renderer`, which also waits there for the tree, grass,
  cloud and volumetrics pipelines started at renderer creation. Meanwhile the editor draws
  its panels and the Scene view says "Compiling shaders…". The heavy terrain and grass
  pipelines compile in parallel (`pipelines::build_parallel`, used by every renderer, tests
  included: same pipelines).
- **Background queues** (`misty_engine::load_queue`): first-in first-out queues with their
  own below-normal-priority workers, `shaders()` and `imports()`; progress per batch,
  `cancel()` (the editor cancels both when a project closes: queued jobs are dropped, running
  ones' results are not published), failed or panicking jobs count as done and report an
  error (the Console shows "Could not import …").
- **Background loads in the renderer** (`Renderer::set_background_loads`, on only around the
  editor's Scene and Game view renders): new mesh and shadow variants and `.shader`
  (shader graph) pipelines compile on the shader queue; an Opaque surface draws with the plain
  Standard variant meanwhile, others are left out; texture decodes, mip chains and uploads run
  on the import queue with the 1×1 defaults bound until they arrive (the bind groups are then
  rebuilt); trees and the grass visibility path skip frames instead of waiting for their
  pipelines. The variants most scenes use (plain, double-sided, cutout and their shadow
  casters) are queued as soon as the renderer is ready. Thumbnails, material previews, probe
  bakes, tests, golden shots and the player render with background loads off: complete
  frames, unchanged images.
- **Status bar**: "Starting the renderer: compiling shaders (N done)", "Compiling shaders
  14/40", "Importing 12/30" with a spinner (Turkish and Arabic too). The old "Preparing
  shaders… (n compiled)" toast is gone. Screenshot runs (`MISTY_SCREENSHOT`) wait for the
  background loads before the shot.
- **Asset scan**: `GuidDatabase::scan` reads and writes the meta files on several threads
  (the GUID decisions stay in the same order): `Editor.AssetDb` 270-300 ms → 85-100 ms on
  SampleProject.
- **Pipeline cache**: already kept across runs in `Library/ShaderCache` (Vulkan, round 35);
  it now fills from the worker threads too.
- **Profiler scopes**: `Renderer.WarmUp`, `Renderer.WaitPrebuilt`, `Import.Model: …`,
  `Import.Texture: …`, `Texture.Importer`, `Render.Prep.*` (shadow mask, screen space,
  volumetrics, glows, weather, clouds, post-processing, draw upload), `Editor.TestRunner`,
  `Editor.SyncCursor`, `Editor.EditPreviews`, `Editor.PollDisk`, `Editor.RefreshAssets`.
- **Tests**: `load_queue` (order, batch progress, cancel drops queued jobs and voids running
  ones, failing and panicking jobs report errors without hanging, parallel workers);
  `render::background_load_gpu_tests` (queued variants swap in and the image then matches a
  render without background loads; a texture decoded in the background arrives and a broken
  one reports its error); `first_open_tests` (`cargo test --release -p misty_editor
  first_open -- --ignored --nocapture`: the built editor opens a fresh SampleProject copy
  with no `Library/` and reports the first frame and the longest frame; plus its parser).
- **Measured** (fresh SampleProject copy, `Library/` removed, freshly named exe so the
  driver's cache is cold, `MISTY_WINDOW_SIZE=1100x650`, RTX 4060 Vulkan, other agents
  building; table in `docs/pending/r44-first-open.md`): SampleScene's first frame 8.7-12.3 s
  → 1.1-1.4 s after start, the longest main-thread stall 8-11.5 s → 150-180 ms; MeadowDemo
  10.3 s → 1.2 s, its longest stall 9.6 s → 0.45 s. Golden shots 20/20.
- **Left**: model imports still load on the calling thread (no placeholder meshes);
  feature and blended variants have no stand-in; a 150-180 ms `Editor.RefreshAssets` once
  after opening; `Editor.State` (0.2 s) before the first frame.

## Round 49 — Sounds: weather, ambience in the demo scenes, footsteps (branch r49-sounds)

The scenes were silent ("is there supposed to be sounds?"). Recordings made in ElevenLabs are
now part of the sample and the engine; details and numbers in `docs/pending/r49-sounds.md`.

- **Assets** (`SampleProject/Assets/Audio/{Ambience,Weather,Footsteps}`, `tools/sample_sounds.py`):
  five 30 s loops (meadow, forest birds, breeze, steady rain, storm) checked for seam clicks
  (none), matched at -24 LUFS under a circular -1 dBFS limiter, Ogg Vorbis (28.8 MB of WAV ->
  2.0 MB, decoded length and seam re-checked); six grass footsteps cut at their onsets (mono
  WAV). About 2.3 MB added.
- **Weather sound**: the bundled steady rain and storm loops (built into the engine) follow the
  rain intensity and crossfade (equal power) as it gets heavy, quieter under cover (rays up from
  the Audio Listener, Under Cover 0.35, with Sheltered Areas on), every level eased so the rain
  fades with the timeline and under roofs. Inspector: Sound, Volume, Under Cover, Rain Loop,
  Storm Loop, Wind Clip, Thunder Clip (empty = generated thunder, delayed by distance).
  Old scenes load and re-save unchanged.
- **Fix**: weather loops and thunder had never played: their absolute `Library/Weather` paths
  were refused as non-project paths (`Audio::play_ambient` now takes them).
- **Ambience** (`AmbienceZones.lua`, `sample_ambience.rs`): NoonValley and MeadowDemo play a
  meadow loop in the open and forest birds from the nearest forest area (3D, the source glides
  to the closest point of the area, surrounds you inside, the meadow loop gives way); MeadowDemo
  adds a breeze. Added by the scenes' generators; scenes regenerated (MeadowDemo keeps the
  user's edits: only Rocks and Ambience are replaced).
- **Footsteps** (`FirstPersonController.lua`): a random grass step (never the same twice in a
  row) on each step on the ground, rate from the stride phase, louder running, softer crouched,
  a landing thump, volume +-15 % and pitch +-6 %, all in the controller's settings.
  `AudioSource.PlayClipAtPoint` takes an optional pitch. GameObject ▸ First Person Player writes
  the clips into projects that lack them.
- **Tests**: rain volume curve (0 when clear, rising, storm takes over, lower under cover),
  driver fades/crossfades/stops, bundled loops decode sample-exact across the loop point, old
  Weather audio loads and new fields stay out of files, footstep picks never repeat and steps
  follow distance, ambience areas match the Lua maths and cover NoonValley's woods.
- **Left**: no thunder recording (generated thunder plays); tree crowns without colliders don't
  muffle rain; one footstep set for every surface.

## Round 50: rain is loud enough under a roof or in a car

- The user found the rain barely audible inside a car. The car roof counts as cover, which cut the
  rain to 35 %, and the rain loops sat at the ambience's -24 LUFS. Now Under Cover defaults to 0.8
  (rain drums on a roof), and the rain loops are mastered louder (steady -17.6, storm -16.3 LUFS,
  `tools/sample_sounds.py` has a loudness per loop). The engine's cached copies are renamed `_v2` so
  old `Library/Weather` copies are not reused.
## Round 48 — Light ▸ Glow: soft glow spread per light (branch r48-light-glow)

- **The harsh glow** around bright lights (Rectangle lights especially) is bloom of the
  light's visible shape, not fog or the Halo: bloom adds its 6 levels at weight 1 (default
  Scatter 0.7), so the bloom image carries 3.6× the emitter's energy, the narrow levels pile
  into a blob as bright as the emitter, and the tail ends at the widest level (a hard edge).
  Volumetric fog in-scattering stays smooth.
- **Light ▸ Glow** (point, spot and Rectangle lights, folded away while untouched; Turkish and
  Arabic): **Intensity** (glow of the visible shape, independent of the lighting), **Spread**
  (0..1: that share of the light's bloom goes to a halo of its own, three Gaussians of the
  screen distance to the projected emitter or around the Halo, normalised to the energy taken
  out of bloom using bloom's real gain, threshold, clamp and tint; the core cools and the tail
  widens with no extra brightness), **Soft Core** (compresses the emitter's visible
  highlights, glow unchanged; dims a Halo's hot centre at the same energy).
- Drawn in the glow pass (`glow.wgsl` `vs_spread` / `fs_spread`, HDR only, halo occlusion and
  fog); emitters get their glow share and Soft Core through `lightmap.zw` of the draw uniform
  (`mesh.wgsl` `light_glow_surface`). Defaults change nothing: the field is left out of files
  while untouched, old files load the same, golden shots 20/20 unchanged.
- **Measured**: on-screen glow energy at spread 0.5 / 1 = 91 % / 72 % of spread 0 (spread 1
  reaches past the frame; the full-plane integral matches to 3 %), glow at the emitter's edge
  0.157 → 0.139 → 0.084. GPU: 16 full-screen spread halos at 1080p add about 1.1 ms (RTX
  4060), about 0.07 ms per light; dim lights get smaller quads.
- **Tests**: glow not saved while untouched / old lights load unchanged; weights carry the
  requested energy; bloom gain; defaults identical; Soft Core keeps the halo energy; GPU energy
  and core/tail test; defaults draw no spread halo. Shots: `glow_spread_shots`
  (`MISTY_GLOW_SHOTS=<dir>`), cost: `glow_spread_benchmark`. Notes:
  `docs/pending/r48-light-glow.md`.
## Round 47 — Smooth LOD transitions: dithered cross-fades (branch r47-lod-fade)

Objects no longer pop when they switch level of detail: inside a band at each switch both
levels draw through complementary screen-space dithers (together they cover every pixel
once: no blending, no sorting), and the shadow casters take the camera's state. Notes:
`docs/pending/r47-lod-fade.md`.

- **One dither** for every cross-fade (`lod_dithered_out` in common.wgsl, mirrored and tested
  in `render/lod.rs`): a 4 × 4 Bayer matrix whose thresholds step by 1/8 per TAA jitter phase,
  so the history averages the two levels instead of keeping a fixed pattern.
- **LOD Groups**: Properties ▸ Fade Mode (None / Cross Fade) and Fade Width (band as a share
  of each switch, 0.1 ≈ a tenth of the distance), now translated. New groups (Add Archetype,
  model import LODs, the sample rocks) default to Cross Fade 0.1; files without a fade mode
  load as None, so old scenes look as before. A LOD Group over a virtual geometry mesh also
  fades out above its cull height. Opaque levels in a band draw with the cutout variant; the
  shadow casters get the same fade, so shadows switch with what the camera sees.
- **Terrain trees**: Tree Rendering ▸ **LOD Fade** (0.1 for new terrains): full mesh →
  lighter mesh → impostor fade over the band just before each set distance (the switch is
  complete at it, as a LOD Group's), and impostors fade out by the draw distance; CPU lists and
  the GPU cull do the same. Terrains saved before it load with 0 (switch at once, as before),
  like LOD Groups without a fade mode: NoonValley is switched on (0.1); MeadowDemo is left as
  saved because its golden views have trees standing right on a switch distance (an oak at
  45 m on High), which any fade band would redraw half-and-half — turn it on there when the
  golden images may be retaken. Tree shadows keep their per-cascade level (texel size),
  which never follows the camera's switch, and the cascades blend across borders.
- **Detail meshes** thin out over the last tenth of their draw distance instead of vanishing.
- **TAA**: the history keeps the nearest depth of its 3 × 3 neighbourhood and rejects itself
  only where it saw a nearer surface (something moved away), so the scattered pixels of a
  cross-fade keep their history (before, every dithered pixel of the nearer level counted as
  a disocclusion and the pattern stayed on screen). With fades off the golden shots are
  unchanged by this.
- **Tests**: LOD selection with bands, complementary coverage at every fade and phase, the
  shader's matrix, the dither walking the jitter cycle, tree bands, old scenes and terrains
  loading, fade settings round-tripping through a saved scene; a GPU dolly across a switch
  (largest one-step change 0.98 of the object instant vs 0.08 cross-faded, no hole in the
  silhouette). Harness `lod_fade_dolly` (fades off / on, frames to disk) and
  `tools/lod_dolly_pops.py` / `tools/lod_dolly_strip.py`.
- **Proof** (NoonValley `treeline`, 72 frames dollying back 0.25 m a frame, High 720p, final
  build): in the tree blocks that switch, the biggest one-frame change of a 32 px block's
  brightness was 40.9 levels (of 255) instant and 4.3 cross-faded; strips in
  `docs/screenshots/r47/`. Golden shots 20/20 (largest mean 0.39, changed 0.03 %).
- **Cost** (High 1920 × 1080, sum of the GPU parts). Two interleaved rounds against main with
  tree fades on in both scenes: NoonValley hero 13.45 → 13.46 ms, inside 11.78 → 11.97 ms
  (trees 1.75 → 1.85), MeadowDemo behind 14.81 → 15.01 ms (trees 0.42 → 0.57), oak 12.62 →
  12.93 ms (trees 0.21 → 0.45). Final build, one round on a loaded machine (±0.6 ms noise):
  trees part NoonValley hero 0.71 → 0.71, inside 1.96 → 2.01 ms; MeadowDemo (tree fade off as
  saved) unchanged at 0.42 / 0.21 ms.
- **Left**: no time-based fade for camera cuts (a jump lands in the new state at once); the
  dither still shows as fine noise on a crown edge against the sky while it is in a band
  (TAA averages it, not fully, and not on the screen border).

## Round 45 — Cheaper grass on the GPU, grass LOD without pops (branch r45-grass-gpu)

### Done (docs/pending/r45-grass-gpu.md)
- **Grass visibility card target in 8 bits** (`grass_vis.rs`, `grass_vis.wgsl`): triangle,
  facing and coverage in an Rgba8Unorm target (4 bytes a sample, was 8): the 4× MSAA raster
  and the resolve move a third less memory. Same picture (mean 0.07/255).
- **Grass LOD fades** (`grass_blade.wgsl`, `grass_cull.wgsl`, `grass_prepass.wgsl`,
  `terrain_grass.rs`):
  - Fine → coarse cards: chunks now switch at 2.5 LOD distances (was 1.5), and the fine cards
    slide their rows onto the coarse card's over the last 5 % before it, so the switch shows
    nothing (it showed as a band across the meadow: forced-switch difference 0.121 →
    0.003/255). At 1.5 the morphed blades were still large on screen and pushed
    `meadow_low_close` over the golden limit.
  - Draw-distance edge: every blade's width fades with the draw-distance fade (the flat fans
    kept their width past it to the end of the last chunk), and nothing past the draw
    distance is drawn.
  - Screen-space prepass: with a range of 12 m or more (High AO, 20 m) its blades thin out
    over the last quarter of the range instead of ending at whole chunks; the 8 m range of
    the lower AO levels is left as it was.
- Diagnostics of where the grass time goes (table in the pending doc), harness additions:
  `MISTY_GRASS_CLOCK=1` (wind clock running in the timings), `noon_valley_dolly`, test knobs
  `MISTY_GRASS_COARSE_FROM` / `MISTY_GRASS_DROP_BEYOND`.
- High 1920×1080 (final build, interleaved with the base; other jobs shared the GPU, so the
  hero figures are from the one quiet run): NoonValley hero GPU 13.40 → 12.97 ms (grass
  visibility 2.37 → 2.11, shading 2.00 → 1.89, screen-space prepass 1.31 → 1.13); down
  sum of passes 12.77 → 11.91 (visibility 2.48 → 1.73); MeadowDemo 14.33 → 13.66
  (visibility 2.50 → 2.27, shading 1.67 → 1.58, prepass 0.51 → 0.35).
- Checks: fmt, clippy 0 warnings, workspace tests (editor 918, engine 1521 + the hitch test
  that failed under load passes alone), golden shots 20/20 (MeadowDemo 0.19–0.46 against a
  base that already sits at 0.19–0.39 from earlier rounds; cycles 0.00).

### Not done
- Hero still above 12 ms. Tried and dropped: per-pixel composite with sample masks (slower),
  two-segment far cards and one-segment fans (no gain), motion vectors for sample 0 only
  (no gain), a smooth leaf count (changes the turf beyond the golden tolerance). The leaf
  count across a card still steps per tuft, and fans entering a chunk's drawn prefix still
  appear whole (per blade, not rings). Next costs: the near grass shadow map (0.7 ms), grass
  in the prepass, sun shafts (1.0 ms).

## Round 53 — MeadowDemo takes the user's look; meadow golden images retaken (branch r53-meadow-look)

- **Why the meadow golden images changed**: the round 49 merge committed MeadowDemo.scene with
  the user's own look edits from the editor (approved as the new intended look), so the 16
  meadow golden shots drifted (mean 30–78) and `meadow_demo_matches_its_generator` failed.
  Cause confirmed first: with the scene from before the edits (a10eecb7^1) on main's code all
  20 golden shots passed (largest mean 0.39, changed 0.03 %), so nothing in the engine had
  regressed. The 16 meadow references were then retaken on main with round 45's grass
  (`MISTY_GOLDEN=update MISTY_GOLDEN_SHOTS=meadow`); the 4 CyclesRoomDemo images are unchanged.
- **The user's look** (the scene file is the source of truth and was not changed): Auto
  Exposure on (center-weighted, progressive, limits -1.5..3 EV), Depth of Field (Gaussian
  10–30 m), Motion Blur, Screen Space GI (intensity 4) and Misty GI (intensity 1.35); clouds
  coverage 0.54, density 0.93, shape 0.42, ambient 0; fog attenuation distance 300 m with
  noise 0.2; contact shadows opacity 0.85 with 16 samples; sun shafts length 0.78, radius
  0.93. The result is a moodier, hazier, cloud-darkened afternoon with soft distance blur.
- **Generator** (`sample_meadow.rs`): writes those values, and the prefab instances' override
  records as the editor saves them (the rocks' scale overrides; the player's record empty, as
  saved). `write_meadow_rocks` keeps the records too. The round 49 Ambience object is kept.
