# Decisions

Record of choices made without asking, and why.

## 2026-11-03 — Step 1

1. **Unity not installed → docs-based notes.** Only an empty `Unity\Hub` folder exists. `docs/unity-notes.md`
   is written from the official Unity Manual instead of a live editor.
2. **Engine name: "Misty Engine".** Matches the project folder and avoids using Unity's name.
3. **eframe as the app shell.** eframe already wraps winit + wgpu + egui (`wgpu` renderer feature), so we get all
   three requested libraries without hand-writing the glue. The Scene/Game views will render 3D through
   `egui_wgpu` paint callbacks into offscreen textures, which keeps everything in one wgpu device.
4. **egui 0.36 + egui_dock 0.21.** egui_dock is the most mature Unity-style docking crate. Its latest version
   targets egui 0.36, so we pin eframe to 0.36 instead of 0.37.
5. **Cargo workspace (`crates/*`).** Editor binary now; an engine library (ECS, renderer, physics) gets its own
   crate in later steps so games can run without the editor.
6. **Placeholder scene data in step 1.** The Hierarchy and Inspector use a small in-memory list of objects so
   the panels can be laid out and clicked. It is replaced by the ECS in step 3.
7. **Transform tools stay in the main toolbar** (older Unity layout) instead of a Scene-view overlay. It is simpler
   and always visible; an overlay can come later.
8. **Sample project folder.** The editor opens `SampleProject/` by default (or a path passed on the command
   line) and the Project window lists the real files under `SampleProject/Assets`.
9. **Unity studied from screenshots only.** Unity 6.3 is installed on `E:`, but simulated mouse/keyboard input is
   blocked by Windows UIPI (most likely the anti-cheat of a game that was running). The notes combine screenshots
   with the manual.

## 2026-11-03 — Step 2

10. **Left-handed, Y-up, Z-forward world; Euler order Z→X→Y.** Matches what Unity users expect: a camera at
    z = -10 looks at the origin, and positive X rotation looks down.
11. **Engine crate `misty_engine`.** Holds camera math and the wgpu renderer so games can later run without the editor.
12. **Offscreen views.** Scene and Game views each render into a 4x MSAA `Rgba8UnormSrgb` target. egui samples it
    through a non-sRGB *view* of the same texture, so lighting maths stays linear and the picture is not
    gamma-corrected twice.
13. **Grid as a full-screen ray–plane shader** with 1/10/100-unit LOD blending, anti-aliased lines and a distance
    fade (turned off in orthographic views, where the eye sits far behind the pivot). The grid plane follows the
    view: XZ normally, YZ/XY in side/front views and 2D.
14. **Scene camera = pivot + yaw/pitch + distance**, like Unity's scene view. RMB looks around a fixed eye; Alt+LMB
    orbits a fixed pivot. Axis snaps switch to orthographic and restore the previous projection when you rotate
    away, with 0.25 s eased animations.
15. **Tools moved into a Scene-view overlay and Pivot/Global into the Scene toolbar**, matching what Unity 6.3 shows.
    The main toolbar keeps Play/Pause/Step plus Layers/Layout.
16. **Placeholder objects drawn as wire gizmos** (cube, sphere, capsule…) until real meshes arrive in step 5. Camera
    and light objects show billboard icons, and the camera frustum or light rays when selected. Picking is by
    screen distance to the icon or centre.
17. **Headless UI tests with egui_kittest.** Scene navigation is exercised with synthetic mouse/keyboard events
    (look, fly, pan, orbit, zoom, view tool, gizmo snapping, click-select) because live input injection is not
    possible on this machine.
18. **`MISTY_SCREENSHOT=path`** makes the editor save a PNG of its own window after ~45 frames and exit. It is used
    for visual checks.

## 2026-11-03 — Step 3

19. **ECS = hecs.** Lightweight and stable, and needs no reflection framework. A small component registry
    (`scene::registry`) gives each component type-erased add/remove/clone/JSON functions, looked up by name. That
    powers Add Component, undo snapshots and (step 9) scene files.
20. **GameObject semantics on top of the ECS.** Every object has `GameObject` (name, active, static, tag, layer,
    *component order*), `Transform` (local TRS + Euler hint) and `Node` (parent + ordered children). The Inspector
    lists components in the order they were added, like Unity, which a plain ECS would not preserve.
21. **Undo = whole-scene snapshots.** Scenes are small, so cloning is cheap and always correct. Commands snapshot
    before they change anything; Inspector drags and typing are grouped from mouse-down/focus to release into one
    "Modify X" step, like Unity's undo groups. Limit is 200 steps.
22. **Re-parenting keeps the world transform** (Unity's default when dragging in the Hierarchy). Drop zones are
    top/bottom quarter = before/after, middle = make child, empty area = scene root.
23. **New objects** are created at the Scene view pivot (rounded), or at the parent's origin when created from a
    Hierarchy row's context menu. Names follow the "Cube (1)" numbering, unique among siblings.
24. **Replaced the step-1 `inspector.rs` with an `inspector/` module** (header + per-component editors). Reason:
    the placeholder object it edited no longer exists; the panel now works on real components.

## 2026-11-03 — Step 4

25. **Transform handles are drawn and hit-tested in screen space** with the egui painter (no extra GPU pipeline).
    Handle size is a constant 90 px, like Unity's. Move: axes + plane squares (placed in the quadrant facing the
    camera) + free centre. Rotate: axis rings (back halves faded) + outer view ring. Scale: axes with cube tips +
    uniform centre. Transform tool = move + smaller rotate rings + scale centre. The Rect tool shows no handles yet.
26. **Snapping with Ctrl** uses Unity's default increments: 0.25 units, 15°, 0.1 scale.
27. **Multi-object editing:** the Inspector shows components shared by all selected objects, with the active
    object's values. Transform edits apply per changed axis; other components copy only the fields that changed
    (JSON diff through the registry). Name is not multi-editable (shown as "—").
28. **Constrain Proportions (scale lock) is an editor-wide toggle** instead of a per-object flag, until scene files exist.

## 2026-11-03 — Step 5

29. **Forward renderer, one pass per view.** Lambert diffuse + normalised Blinn-Phong specular (smoothness →
    exponent, metallic tints specular), hemisphere ambient taken from the sky gradient. Up to 8 lights. It is not
    full PBR, but it is close enough to the Standard shader's look and cheap.
30. **One directional shadow map** (2048², 3×3 PCF, slope bias + normal offset, front-face culling) for the first
    directional light with shadows. It is fitted to the shadow casters' bounds. Point/spot shadows: not yet.
31. **Selection outline via a mask pass.** Selected meshes (and their children) are rendered into an R8 mask, and a
    fullscreen pass draws a ~2 px orange edge. Like the reference editor, the outline shows through occluders.
32. **Materials are JSON `.mat` assets** in `Assets/` (Standard: Albedo, Metallic, Smoothness, Emission). Edits
    save immediately. Mesh Renderer stores the asset path; `None` = Default-Material. Material edits are not in
    the undo history yet.
33. **Line endings normalised to LF** with `.gitattributes`. Python edit scripts on Windows had been writing CRLF.
34. **Mesh winding:** outward `cross(b-a, c-a)`, which is clockwise on screen in our left-handed setup, so all
    pipelines use `FrontFace::Cw` (covered by a unit test).

## 2026-11-03 — Step 6

35. **Play = scene snapshot.** Play clones the scene, selection and undo history. Stop restores them, so any change
    made while playing is reverted and the undo history is the edit-mode one, as you'd expect from Play mode.
36. **Game loop:** `runtime::update(scene, time)` runs once per editor frame while playing. `Time` follows the
    familiar Time API (time, deltaTime, unscaled, frameCount, timeScale) with deltas capped at 1/3 s. Step = one
    1/60 s frame while paused, and Step in Edit mode enters Play paused. Pause before Play starts paused.
37. **Play Maximized** swaps the dock layout for a single Game tab until Stop; Play Focused focuses the Game tab;
    Play Unfocused leaves the layout alone.

## 2026-11-03 — Fixes after checkpoint 3

38. **Hard vs Soft shadows:** Hard = one bilinear depth compare (crisp edge); Soft = 5×5 PCF over a 1.5-texel
    footprint. Both are per light, following the Light's Shadow Type.
39. **Spot and point shadows** use a shared 16-layer 1024² depth array: a spot light takes 1 layer (perspective
    frustum = cone angle), a point light takes 6 (one 92° frustum per cube face, picked by the major axis in the
    shader). Lights beyond the 16-layer budget render without shadows. One directional light keeps the 2048² map.
40. **Shading moved to GGX specular + sky reflections** (Fresnel with roughness-based blur and surface reduction,
    like the Standard shader's approximation), so Smoothness and Metallic now have a clearly visible effect. The sky
    gradient is shared with the skybox shader so reflections match the background.

## 2026-11-03 — Step 7

41. **Physics = rapier3d 0.36** (pure Rust, deterministic, CCD, trimesh/convex). rapier uses glam 0.33 through
    glamx while we use 0.34, so values cross the boundary as plain arrays in `physics.rs`.
42. **The physics world is built when Play starts** from active objects. A Rigidbody gives a dynamic or kinematic
    body; colliders attach to the nearest Rigidbody on themselves or a parent (compound colliders), and colliders
    without one are static. Sizes include world scale. Mass comes from Rigidbody.mass (collider density 0).
43. **Fixed timestep 0.02 s** (the usual default) with an accumulator, max 8 steps per frame. Kinematic bodies follow
    their Transform before each step; dynamic bodies write their pose back after.
44. **Friction/Bounciness live on each collider** (default 0.6 / 0) instead of a separate physics-material asset.
    That's simpler for now; an asset can replace it later.
45. **Primitives get colliders like the reference editor:** Cube→Box, Sphere→Sphere, Capsule/Cylinder→Capsule,
    Plane/Quad→Mesh. A non-convex Mesh Collider on a dynamic Rigidbody falls back to convex, with a Console warning.

## 2026-11-03 — Step 8

46. **Scripting language = Lua 5.4 (mlua, vendored).** C# isn't available, and Lua is the most widely used
    embedded game-scripting language: fast, small, hot-reloadable, with good editor tooling. A script returns a
    class table; plain values are Inspector fields and functions are lifecycle callbacks, mirroring
    MonoBehaviour. (Rhai was the pure-Rust alternative; Lua won on familiarity and speed.)
47. **Safe engine access from scripts:** for one game frame the scene and physics are moved into the Lua state's
    app data (`World`), and userdata (Transform, GameObject, Rigidbody) hold only entity ids and borrow the world
    per call. No raw pointers, and a destroyed object gives a Lua error instead of a crash.
48. **Frame order:** Start (first frame) → for each fixed step: FixedUpdate, physics step, collision/trigger
    callbacks → Update → LateUpdate → Destroy queue.
49. **Forces are applied as velocity changes** using the Rigidbody component's mass and the fixed timestep
    (ForceMode Force/Impulse/VelocityChange/Acceleration). This works from `Start`, before the physics engine has
    computed mass properties.
50. **Several scripts per object** live in one `Scripts` component (a list); the Inspector shows each as its own
    "Name (Script)" section. Field overrides are stored per script; missing values use the script's default.
51. **Hot reload in Edit mode:** script files are polled for changes twice a second; fields are re-read and
    syntax errors go to the Console. While playing, scripts keep the version they started with (like the
    default "recompile after finished playing").
52. **Game input only while the Game view has focus** (click it; Play Focused gives focus on Play). Plain-key editor
    shortcuts are suspended then, so WASD don't switch tools during play.
53. **Shadow gap fix:** the shadow pass now renders light-facing faces (back-face culling) instead of front-face
    culling, which left contact points unshadowed. Acne is avoided with a normal offset of ~1.5–3 shadow texels
    (world-size texel computed per light) plus a small slope/constant depth bias.

## 2026-11-03 — Step 9

54. **Scene format = pretty JSON (`.scene`)**, chosen for git-friendly diffs, like text-serialised scenes.
    Objects are listed depth-first in Hierarchy order with a parent index; every component goes through the
    registry (Node links are rebuilt). Keys are sorted and floats are written in their shortest f32 form, so saving
    an unchanged scene is byte-identical. Unknown components and missing fields load with warnings instead of
    failing, and a `version` field guards against files from newer builds.
55. **Native file dialogs (rfd)** for Open / Save As, limited to the project's `Assets` folder. Saving is blocked
    in Play mode, as are New/Open.
56. **"Scene Has Been Modified" prompt** (Save / Don't Save / Cancel) before New Scene, Open Scene and quitting;
    closing the window is intercepted the same way. Loading a scene clears selection and undo history.
57. **Per-user editor state in `<Project>/Library/EditorSettings.json`** (recent scenes), git-ignored like a Library
    folder. The editor reopens the most recent scene on startup, falling back to `Assets/Scenes/SampleScene.scene`.
58. **Window title** shows "Project - Scene* - Misty Engine", with `*` while unsaved.

## 2026-11-03 — Emission fix

59. **HDR rendering + bloom.** Scenes render into an `Rgba16Float` MSAA buffer (values above 1 survive), then
    bloom and a composite pass write the 8-bit display image. Emission Intensity is now a linear multiplier applied
    after converting the colour to linear (it used to go through the gamma curve and clip at 1).
60. **Bloom only from emission:** meshes write their emission into a second "glow" render target in the same pass,
    and bloom (13-tap downsample + tent upsample chain) uses only that buffer. Brightly lit white surfaces don't
    bloom, and Intensity directly controls how far an object glows.
61. **Hue-preserving highlight compression** in the composite instead of per-channel clipping: over-bright orange
    stays orange and whitens gradually. There's no global tonemapper, so normal scenes look the same as before.

## 2026-11-03 — Project Settings

62. **Project Settings live in `<Project>/ProjectSettings/ProjectSettings.json`**, one pretty JSON file with
    `physics` and `time` sections, committed with the project (unlike `Library/`). Every field has a default
    (`serde(default)`), so older or hand-trimmed files keep loading; invalid values (zero timestep, NaN) are clamped.
    A missing file means defaults and isn't written until something changes.
63. **A floating egui window instead of a dock tab**, like the reference editor's utility window, and so the
    dock layout/persistence code doesn't need a new tab kind. Changes save immediately (no Apply button).
64. **Fixed Timestep is stored in `physics` but shown under Time**, where users expect it; physics is the only
    thing that consumes it. "Max Steps Per Frame" (no direct equivalent) sits under Physics.
65. **No project-wide default friction/bounciness:** colliders already own theirs (decision 44), so a second
    default would be confusing until physics-material assets exist.
66. **Settings are read when Play starts** (physics world, fixed step). Time Scale and Maximum Allowed Timestep also
    update the running clock immediately; gravity/fixed-step edits apply on the next Play.

## 2026-11-03 — Window layouts

67. **Layouts live in their own `<Project>/Library/Layouts.json`** (current arrangement, its name, named user
    layouts) instead of a field in `EditorSettings.json`. Dock trees are large and change often; a separate file
    keeps the recent-scenes file small and lets the two be written independently. It is git-ignored with the rest
    of `Library/`.
68. **Saving is change-driven and debounced:** once a second the layout is serialized and written only if the text
    differs from what is on disk; exit and user-layout edits write immediately. Screen rects are zeroed before
    serializing (egui recomputes them every frame), so resizing the window doesn't rewrite the file and fresh
    layouts don't contain infinities that JSON can't store.
69. **Built-in layouts** approximate the same panel arrangements: 2 by 3 (Scene over Game, then Hierarchy,
    Project+Console and Inspector columns), 4 Split (2×2 grid of Scene/Game/Hierarchy/Project, Inspector right),
    Tall (full-height Scene+Game, Hierarchy over Project+Console, Inspector), Wide (Scene+Game on top with
    Hierarchy | Project+Console below, full-height Inspector). Every built-in contains all six tabs once. User
    layouts may not reuse a built-in name; saving an existing name replaces it.
70. **Main window size/position via eframe's `persistence` feature** (stored by eframe in the OS app-data folder,
    since window placement is per machine, not per project). It restores the maximized flag too, so it works
    with `with_maximized(true)` on first run. egui's own memory isn't persisted (`persist_egui_memory = false`):
    editor state belongs in the project.

## 2026-11-03 — Prefabs

71. **Prefab = scene file format with one root** (`.prefab`, pretty JSON). Scenes and prefabs share the object list
    code (`scene/file.rs`), so a prefab is readable, diffable and loads with the same tolerant warnings.
72. **Instance = full copy + `PrefabLink { path }` on the root** (registry name "Prefab", not in Add Component or the
    Inspector list). The link is an ordinary component, so it is saved in scenes and undo snapshots for free.
    Children aren't linked; "part of an instance" = has a linked ancestor.
73. **Overrides = whatever differs from the asset**, computed by comparing the instance's serialised subtree with the
    asset (both canonicalised through a scene round-trip). The root's name, position and rotation are per-instance
    and never count, matching the default overrides users expect. Revert/Apply All are disabled without overrides.
74. **Revert and Apply All rebuild instances** from the asset (keeping place in the Hierarchy, name, position,
    rotation). Simple and correct for the instance being applied; other instances lose their own overrides, which
    is noted as a mismatch until per-property overrides exist.
75. **Prefab assets are cached in the AssetDatabase** keyed by path and file time, because the Inspector compares the
    selected instance with its asset every frame.
76. **New prefabs never overwrite**: dragging "Crate" into a folder that has `Crate.prefab` creates `Crate 1.prefab`
    (no modal Replace dialog in nonstop mode; data is never lost).
77. **Dropped prefabs are placed at the Scene view pivot** (like GameObject ▸ Create), also for Hierarchy drops at
    the root; dropping onto a Hierarchy row parents the instance under it at the prefab's own local position.
78. **Lua `Instantiate` copies are unlinked "Name(Clone)" objects** (runtime instances aren't prefab-connected).
    Their physics bodies are added with `Physics::add_subtree`; scripts get `Awake` right after the callback that
    called `Instantiate` (not inside it, since the host can't be re-entered from Lua), and `Start` before their
    first `Update`. A chain of more than 64 nested Awake→Instantiate rounds is stopped with a Console error.

## 2026-11-03 — Standalone player and Build Settings

79. **Player = plain `winit` 0.30 + `wgpu` surface, no eframe** (`crates/misty_player`). The player has no UI, so
    egui would only add a second renderer and an event loop it doesn't need; winit is already in the dependency
    tree (same version eframe uses). The engine renders to its usual offscreen `RenderTarget`; a fullscreen
    blit copies the target's gamma-encoded image to a non-sRGB view of the surface (`remove_srgb_suffix`), so the
    window shows exactly what the Game view shows. Resizing recreates the target at window size (no letterbox).
80. **Scene → renderer code moved to `misty_engine::render_scene`** (mesh draws, lights, headlight, game camera,
    `GameView`), with a small `MaterialSource` trait implemented by the editor's `AssetDatabase` and the new
    engine `AssetCache` (loads every `.mat` under `Assets/` once — a build never changes its assets).
81. **Build Settings in `ProjectSettings/BuildSettings.json`** (scene list with enabled flags, product name,
    width/height, fullscreen), version-controlled like the Project Settings. Build index = position among the
    enabled scenes, like the familiar list. An empty product name means the project folder's name.
82. **A build is a folder**: `<Product>.exe` (the player, renamed), plus copies of `Assets/` and `ProjectSettings/`
    (replaced on each build so deleted assets don't linger). The player plays the folder it sits in, so no packing
    format is needed yet. Building into the project's own `Assets`/`ProjectSettings` is refused. With no enabled
    scenes the open (saved) scene is added first, matching the reference editor's "build the open scene".
83. **The player executable is found next to the editor's** (`current_exe()` dir); if it is missing the Console says
    to run `cargo build -p misty_player --release`. Building the player from the editor would need cargo and the
    source tree at runtime, which an installed editor won't have.
84. **Build always asks for a folder; Build And Run (button, File menu, Ctrl+B) reuses the last folder** (stored per
    user in `Library/EditorSettings.json`) and asks only the first time.
85. **`SceneManager.LoadScene` / `Application.Quit` are requests handled after the frame** (`Runtime::take_requests`),
    since the script host can't replace the world it is running. The last LoadScene of a frame wins; scenes must be
    enabled in Build Settings (by file name, path or index) or a Console error explains why, as users expect. A
    load restarts physics and scripts (`Awake`) but `Time` keeps running. In the editor the loaded scene replaces
    the Play-mode scene (and the window title's scene) until Stop restores the edit scene and its path.
86. **Player key names come from physical key codes** (layout independent) and use the familiar names
    ("left shift", "page up", "-"); the editor's egui-based capture keeps its names, which match for letters,
    digits, arrows, space, return, escape, F-keys and modifiers. Release players use the Windows GUI subsystem
    (no console); output goes to stderr and `Player.log` next to the executable.

## 2026-11-03 — Audio

87. **kira (0.12) for audio output, not rodio.** kira is built for games: every playing sound has a handle whose
    volume, playback rate and stereo panning can be changed (with short tweens, so per-frame 3D updates don't
    click), plus loop regions and pause/resume. rodio's `Sink` has volume/speed but no live panning, which 3D sound
    needs. kira decodes wav/ogg/mp3 through symphonia (only those formats are enabled).
88. **Spatial mixing is our own math, not kira's spatial tracks:** `audio::spatial` computes rolloff (Logarithmic =
    `min / distance`, holding its level past Max Distance; Linear = fade to silence at Max Distance) and pan
    (direction to the source · listener right), then blends 2D/3D by Spatial Blend. It is pure, unit-tested,
    backend-independent, and matches the familiar Inspector settings directly.
89. **Swappable backend, device opened lazily.** `Audio` talks to an `AudioBackend` trait: `KiraBackend` (real
    device) or `NullBackend` (records calls, never makes sound). The device is opened on the first sound, so tests,
    scenes without audio and machines without a sound card never touch it; failure logs one Console warning and the
    game keeps running silently. Tests and `ScriptHost::start` use the null backend.
90. **`Audio` lives in the scripting `World`**, next to the scene and physics, so Lua `AudioSource` methods can
    start/stop voices synchronously and `isPlaying` is correct immediately after `Play()`. Property setters write
    the component; the per-frame update pushes them to playing voices.
91. **Play = restart, Pause/UnPause = hold/resume**, `isPlaying` is false while paused or after the clip ends.
    `PlayOneShot(path [, volume])` is a method on the source (its mix and position) and is not counted by
    `isPlaying`. Sources stop when disabled, deactivated or destroyed.
92. **Editor Pause pauses audio; Mute Audio = volume 0** (voices keep running, so unmuting resumes mid-clip). The
    Audio is created already muted when Mute Audio was on before Play, so play-on-awake sounds don't blip.
93. **Clip preview uses its own `Audio`** in the editor state (device opened on the first ▶), stopped when Play
    starts. The Inspector shows WAV format/length read from the header; other formats show the file size.
94. **Sample clip is generated, not downloaded:** `audio::wav::sine_beep` writes a 0.3 s 660 Hz mono 16-bit WAV;
    a test checks the committed file (regenerate with `MISTY_WRITE_SAMPLES=1 cargo test sample_clip`).

## 2026-11-03 — Textures and 3D models

95. **`MeshFilter.mesh` is `Option<MeshRef>`, an untagged enum of `PrimitiveMesh` or an asset string**
    (`"Assets/Models/Ship.glb#0"`). Primitives keep serialising as `"Cube"`, so old scenes and prefabs load and save
    byte-identically; asset paths always contain `Assets/`, so they never collide with primitive names.
96. **One process-wide model cache in the engine (`misty_engine::models`)** keyed by project-relative path, plus a
    process-wide project root (`assets::set_project_root`, set when the editor opens a project). Renderer, physics,
    picking, gizmos and the Inspector all need imported meshes by path; a shared cache avoids threading an asset
    database through every API. Edit mode polls file times (twice a second) and bumps a generation counter that GPU
    caches compare. Tests use absolute paths so they don't depend on which root another test set.
97. **Import mirrors X** (positions, normals, node translations; quaternions `(x, -y, -z, w)`) and swaps triangle
    winding: glTF/OBJ are right-handed, the engine is left-handed, and this is the conversion users expect (a model
    authored facing +Z still faces +Z). OBJ `v` is flipped since our UV origin is top-left (like glTF).
98. **A glTF mesh's primitives become separate meshes** (`#index` counts primitives across the file); a node with
    several becomes one object per extra primitive as children, because a Mesh Renderer has one material slot.
    A model with a single node becomes one object; otherwise a root named after the file holds the nodes.
99. **Tangents are always generated** (per-triangle UV derivatives, Gram-Schmidt, handedness in `w`) and glTF
    tangents are ignored, so primitives and both formats behave the same. The bitangent points to *decreasing* `v`
    (up in the image), i.e. normal maps use the OpenGL / glTF green-up convention. Mirrored model matrices flip
    the handedness in the shader.
100. **Mipmaps are built on the CPU** with a 2×2 box filter (sRGB maps averaged in linear light) and uploaded in one
    `create_texture_with_data` call: no extra compute/blit pipeline, deterministic, and testable without a GPU.
    Images larger than 4096 are scaled down on load.
101. **Textures bind per material at group 2** of the lit pipeline (albedo, normal, one trilinear 8× anisotropic
    repeat sampler); bind groups are cached by (albedo, normal) path pair, with 1×1 white / flat-normal defaults.
    Shadow and mask passes are untouched. Tiling/offset and normal strength live in the per-draw storage buffer
    (normal strength 0 = no normal map, so the vertex normal is used exactly).
102. **glTF materials are extracted once to `Model_Material.mat` beside the model** (embedded images to
    `Model_Material_Albedo.png` / `_Normal.png`) when the asset database scans the model, and existing files are
    never overwritten, so users can edit them like Unity's extracted materials. Base colour factors are converted
    from linear to sRGB; smoothness = 1 − roughness.
103. **Sample textures are generated by code** (`misty_editor` test module `sample_assets`, an ignored test writes
    them; a normal test checks the committed files still match), so the repo holds no downloaded art.

## 2026-11-03 — Animation

104. **Legacy-style `Animation` component instead of an Animator + state machine.** A clip list, a default clip,
    Play Automatically and Wrap Mode cover "make this object bob/blink/open" without an Animator Controller asset,
    state graph, parameters or transitions, which would be a large separate editor. Clips, curves and the Animation
    window are the parts both systems share, so an Animator can be added on top later without changing `.anim`.
105. **`.anim` is readable JSON:** `length`, `loop`, `sample_rate`, `curves` (`path`, `property`, `keys`) and
    `events`. Properties are named like `"Transform.position.y"`, `"Light.color.r"`, `"GameObject.active"`; the
    tangent mode is omitted when it is Auto. Loading sorts keys, drops duplicate times and grows `length` to the
    last key, so hand-edited files are safe.
106. **Curves are cubic Hermite with per-key tangent modes.** Auto = Catmull-Rom slope, flattened at the first/last
    key and at local extremes (no overshoot, like clamped auto tangents); Linear = straight to the neighbours;
    Constant = hold until the next key. Tangents are not stored (no weighted/free/broken tangents yet). Booleans
    (`GameObject.active`) always step and are on at ≥ 0.5.
107. **Curves bind by child name path** relative to the object with the Animation ("" = itself, "Arm/Hand"), so a
    clip works on any object with the same child names; renamed/missing children only skip their curves (the
    window shows them as "(Missing!)"). Rotation is animated as Euler angles per axis (through `euler_hint`), so
    0→360 keys spin a full turn, as users expect from the Inspector.
108. **Animated properties: Transform position/rotation/scale, Light intensity/range/colour, GameObject active.**
    Material colour is not animatable: materials are shared assets and there are no per-renderer overrides
    (material property blocks) yet; animating the asset would change every object using it.
109. **Playback lives in the scripting `World` (`AnimationPlayer`)**, like audio, so Lua `Play/Stop/time/speed`
    act immediately. Animations are sampled after `Update` and before `LateUpdate` (Unity's order), clips are
    loaded once per Play session from disk, and events are dispatched to the animated object's scripts by
    function name. Wrap Mode Default follows the clip's Loop flag; Once stops at the end and keeps the last pose;
    `Stop()` rewinds without resetting the pose. `time` is unwrapped (keeps growing while looping).
110. **Speed is per Animation component, not per clip state** (`anim.speed`), and is kept across `Play` calls.
    There is no `anim["Clip"]` state indexing, cross-fading, blending or layers.
111. **Editor preview backs up exactly the values it overrides.** The first time a curve touches a property while
    previewing, its current value is stored; leaving the preview (Preview button, selecting another object, Play,
    Save, New/Open Scene, Quit) writes the originals back. The preview pose is therefore never saved or played.
    Saving while previewing ends the preview first (Unity keeps previewing while saving the original values).
112. **Record mode compares values instead of hooking every editor.** Right after sampling, the values of every
    animatable property of the animated object and its children are remembered; after the UI pass, anything that
    changed (Inspector fields, gizmos, the active checkbox, commands) becomes a key at the playhead (snapped to the
    sample rate). A property recorded for the first time at t > 0 also gets its original value at 0. Undo/Redo
    frames are excluded so undoing never records keys.
113. **Key edits undo in the normal history:** an undo snapshot may carry one clip (the window's clip). Continuous
    edits capture it when the interaction starts, so dragging a Transform field in Record mode is one step that
    restores both the scene and the keys. Clips are edited in memory (`ClipStore`) and written to their `.anim`
    file when the interaction ends, so there is no separate "save clip" step.
114. **Create New Clip makes `Assets/Animations/<Object>.anim` without a file dialog** (numbered if taken) and adds
    an Animation component if needed. A native save dialog would block tests and the clip can be moved later.
    Add Property keys the current value at the start and at the end of the clip, like Unity.
115. **The Animation window is one tab next to Project/Console in every built-in layout** and opens with Ctrl+6
    or Window ▸ Animation ▸ Animation. Delete removes selected keys when the last click was inside the window.
    Curves view draws the shown curves (all, or the rows clicked in the property list) in one auto-fitted value
    range; there is no tangent-handle editing, curve presets or separate zoom per axis yet.
116. **Sample clips and the AnimationDemo scene are generated by code** (`sample_animation` test module) and a test
    checks the committed files and plays them, like the texture samples.

## 2026-11-03 — Runtime UI

117. **Rect Transform lives beside the Transform**, not instead of it. Every object keeps its Transform (hierarchy,
    scene files, prefabs and gizmos all rely on it); UI layout ignores it and uses the Rect Transform only. The
    Inspector shows the Rect Transform in the Transform's place when an object has one. Rect Transform maths
    follows the familiar model exactly (anchors min/max, pivot, anchored position, size delta, y up), so values
    typed by users mean the same thing; layout is pure code in `misty_engine::ui::layout` with unit tests.
118. **UI components live in their own file** (`scene/ui_components.rs`, re-exported from `scene`) rather than at the
    end of `components.rs`, to keep parallel feature work from colliding; registry entries sit right after
    Transform. Canvas Scaler is a separate component like the original (GameObject ▸ UI ▸ Canvas adds both).
    Its default is Scale With Screen Size, 800×600, match width — what the Canvas menu item should give.
119. **Only Screen Space - Overlay canvases.** A root canvas is laid out to the game image size (the Game view's
    pixel size, or the player's window in physical pixels) divided by the scale factor. Nested canvases just add a
    rect to their root canvas. Root canvases draw by Sort Order, then Hierarchy order; children draw after parents.
120. **Button On Click targets are stored as object references.** In memory a `ButtonCall` holds an `Entity`;
    scene/prefab files store the target's index in the object list (like parents), remapped on save/load by a
    small table in `scene::file` (`OBJECT_REFS`). Loading now spawns all objects first so references can point
    forward. References outside a saved prefab become `null`. Duplicating keeps the original target.
121. **On Click calls a Lua method by name on every script of the target object** that defines it (closest to
    picking a component method in the original's event list). A missing method logs a Console warning.
122. **UI events run once per frame, after `Start` and before `FixedUpdate`/`Update`**, like an event system
    updating early. Hover = topmost raycast-target Image/Text under the pointer, bubbled up to the nearest Button;
    a click is press + release over the same interactable button. Buttons tint their own Image (Colour Tint only,
    no fade, no Selected state / keyboard navigation). No EventSystem object is needed or created.
123. **Text uses one bundled font: Ubuntu Light from `epaint_default_fonts`** (already a dependency of the editor's UI
    library, so no font file is added to the repo), rasterised with `ab_glyph` into a 1024² coverage atlas at the
    on-screen pixel size, so text is crisp at every Canvas Scaler factor. When the atlas fills it is cleared and the
    frame rebuilt. Vertical overflow is always Overflow (no truncation); horizontal is Wrap or Overflow.
124. **UI renders as its own pass (`render::ui::UiRenderer`) on the render target's final sRGB image**, after the 3D
    frame and before the editor shows it / the player blits it, so both share the same code. Vertices are built on
    the CPU in clip space (no uniforms); one pipeline handles colour quads, textures and glyphs (`mask` attribute).
    Colours are sRGB like every other colour field and are blended in linear space by the sRGB target.
125. **Scene view shows UI as outlines**: every overlay canvas as a rectangle at one world unit per screen pixel with
    its lower-left corner at the origin (the familiar huge canvas), selected UI rects in blue with anchor crosses.
    Frame Selected frames a UI object's rect. UI graphics themselves are not drawn in the Scene view.
126. **Lua UI API**: `GetComponent("Text"/"Image"/"Button")`, `Text.text/fontSize/color/enabled`,
    `Image.color/texture/raycastTarget/enabled`, `Button.interactable`, and a `Color` type (`Color.new`, named
    colours, `Lerp`, `*`). Assigning a number to `text` shows it as text.
127. **UIDemo sample scene is generated by a test** (`sample_ui.rs`, built with the GameObject ▸ UI commands) and
    another test plays it headlessly and clicks the button; the scene is not added to Build Settings so the player
    keeps starting SampleScene.
