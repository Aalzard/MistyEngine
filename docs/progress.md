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
