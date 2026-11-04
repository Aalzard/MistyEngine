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

## 2026-11-03 — Particle System

128. **Settings and live particles are two components.** `ParticleSystem` (registered, saved, `#[serde(default)]`)
     holds the modules; `ParticleEmitter` (runtime only, not in the registry) holds particles and play state on
     the same entity. Because `Scene::clone`, scene files and undo snapshots only copy registered components,
     particles never end up in files, undo history or the Play-mode copy, with no special cases.
129. **The component lives in `misty_engine::particles` and is re-exported from `scene::components`**, next to
     its pure simulation (`sim`, `shape`, `rng`), instead of growing the already 700-line `components.rs`.
130. **Simulation is deterministic and frame-rate independent.** A seeded PCG32 per emitter (seed 0 = "Auto
     Random Seed", a fresh seed each Play); births are spread across the frame and pre-aged, so a 4 fps frame
     emits the same count and spacing as 60 fps. Gravity is the project's physics gravity × Gravity Modifier
     (default gravity in edit-mode preview). Prewarm runs one cycle at 30 Hz.
131. **Play semantics follow the familiar API:** Play resumes a paused system or starts a new cycle (keeping live
     particles); Stop stops emitting and lets particles die (`isPlaying` false at once); Pause freezes; Clear
     removes particles; Emit works in any state. A non-looping system stays "playing" until its last particle
     dies. The Scene-view overlay's Stop also clears, like the editor's own overlay.
132. **Play On Awake is applied lazily**: the first `particles::update` (or first script access) that finds a
     system without an emitter creates one, playing if Play On Awake is set. This covers scene objects and
     objects created by `Instantiate` with one rule. Inactive objects neither simulate nor draw (they are not
     reset when reactivated — a simplification).
133. **HDR brightness through Renderer ▸ Intensity.** There are no particle materials yet, so the Renderer module
     has a texture slot, blend mode (Alpha Blended / Additive) and an HDR intensity multiplier. The fragment
     writes `colour × saturate(max(rgb) − 1)` to the glow target, so only the part above 1 blooms (a soft
     threshold that matches how emissive meshes glow). The glow target now blends like the colour target.
134. **Billboards are instanced quads (6 vertices from `vertex_index`)** built from the camera's right/up taken from
     the inverse view-projection; drawn after the grid (they don't write depth, so the grid would cover them) and
     before gizmo lines, depth-tested `LessEqual`. Systems are ordered back to front by emitter position and
     alpha-blended particles are CPU-sorted by distance (view depth in orthographic views); additive ones are not
     sorted. Untextured particles use a procedural soft round sprite.
135. **Edit-mode preview = selected systems and their child systems**, auto-playing on selection regardless of Play
     On Awake; emitters of deselected systems are dropped, and an undo/redo restarts the preview. The overlay is
     an egui area in the Scene view's lower-right corner (Play/Pause, Restart, Stop, Playback Speed, Playback
     Time, Particles).
136. **GameObject ▸ Effects ▸ Particle System** creates the object rotated (−90, 0, 0) so the default cone emits
     upwards, as users expect. The emission axis is local +Z for every shape.
137. **`MISTY_SELECT=Name`** selects an object at editor startup; used with `MISTY_SCREENSHOT` to verify the
     Inspector and particle preview automatically.
138. **The ParticlesDemo sample is generated by code** (`sample_particles` test module, ignored writer test + a
     check test), like the textures sample: a campfire with additive fire and sparks, alpha-blended smoke, a
     point light, new `Wood.mat` and `Dirt.mat` materials and a `SparkBurst.lua` script (Space → `Emit`).

## 2026-11-03 — Editor workflows (snapping, clipboard, multi-edit, Project file operations)

139. **Grid and Snap settings live in `Library/SnapSettings.json`** (per user and per project, like the
     other editor preferences in `Library/`): grid size 1 per axis, move increment 0.25 per axis, rotate 15°,
     scale 0.1, grid snapping off. Edit ▸ Grid and Snap edits them and saves on every change; the Scene
     toolbar 🧲 toggle flips grid snapping. The window also has Align Selected to Grid (one undo step).
140. **Increment snapping (Ctrl) rounds the drag delta along each handle axis by that axis' increment**;
     rotation rounds the angle, scale rounds the factor. **Grid snapping rounds the handle position** (active
     object pivot, or the centre with Pivot ▸ Center) to the world grid on the dragged axes only, and the
     rest of the selection moves by the same delta. Like the familiar toolbar, grid snapping needs the Global
     handle: the toggle is greyed out with Local, and rotate/scale are not affected by it.
141. **Surface snapping (Ctrl+Shift while dragging the centre move handle) hits mesh bounds boxes**, the same
     approximation Scene-view picking uses, and places the pivot on the hit point (no align-to-normal). The
     dragged objects and their children are ignored.
142. **Vertex snapping: hold V.** The move handle jumps to the selection's vertex nearest the cursor; dragging
     moves that vertex onto the nearest vertex of another mesh within 40 px of the cursor (vertices are read
     from the mesh data, so this is exact for primitives and imported models).
143. **Component context menu** (⋮ button, or right-click on the component title): Reset, Remove Component,
     Move Up / Move Down, Copy Component, Paste Component As New, Paste Component Values. Reset, Remove and
     both Pastes apply to every object the Inspector edits, each as one undo step; Move Up/Down is disabled
     while several objects are edited and at either end of the list (Transform always stays first). Paste As
     New only adds a component an object doesn't already have (one component of each type per object here).
     The commands moved from `commands.rs` to a new `component_ops.rs`.
144. **Mixed values ("—") come from a JSON diff.** Before a component's fields are drawn, `inspector::mixed`
     compares its serialised form on every target and keeps the differing paths ("fov", "center/1",
     "surface/friction") in a thread-local set; editors wrap their widgets with `mixed::drag/slider/checkbox/
     text` keyed by the field's JSON name. Mixed numbers show "—", mixed checkboxes are indeterminate, mixed
     combos show "—"; typing a value or dragging sets it on all targets (existing propagation of changed
     fields). Covered: Transform (per axis), the GameObject header (name, active, static, tag, layer),
     component enable toggles, Camera, Light, Mesh Renderer, Audio Source, Rigidbody and the colliders. Colour
     swatches, Particle System, UI components, Animation and script fields still show the active object's
     value (edits still apply to all).
145. **Asset references are paths, so renames/moves rewrite text.** After a rename or move, every quoted
     occurrence of the old path (`"Assets/Old.mat"`) or of a moved folder's prefix (`"Assets/Old/…`) is
     replaced in all `.scene`, `.prefab` and `.mat` files under Assets and in
     `ProjectSettings/BuildSettings.json`. Plain text replacement keeps the files' formatting and touches
     nothing else (our writer never escapes `/`). In memory the editor rewrites the open scene (every registered
     component through its JSON), the undo history, the Play-mode snapshot, Build Settings (reloaded), the open
     scene path and name (a scene's name is its file name), recent scenes, open animation clips, the Animation
     window clip, the Inspector asset and the Project selection. Lua source is not rewritten (scripts that load
     scenes by path must be edited by hand, as with string paths in the familiar engine).
146. **Delete moves to the OS recycle bin / trash** with the `trash` crate after a "Delete selected asset?"
     dialog; nothing is ever hard-deleted. Tests swap the trash for a temp folder so running them doesn't fill
     the user's Recycle Bin.
147. **Project window keyboard focus follows the last click** (like the Hierarchy): while it has focus, Ctrl+D,
     F2 and Delete act on the selected asset instead of GameObjects, and Ctrl+C/X/V are left alone. New assets
     (Create ▸ Folder / Material / Lua Script / Scene / Animation) are selected and start an inline rename, like
     the familiar Project window. The window keeps a single selection.
148. **Search type filters**: `t:Material`, `t:Script`, `t:Scene`, `t:Prefab`, `t:Texture`, `t:Model`,
     `t:AudioClip`, plus `t:AnimationClip` and `t:Folder`; case-insensitive, several `t:` are OR-ed and any
     other words must all appear in the name.
149. **GameObject Copy/Cut/Paste goes through the system clipboard as text**: a `MistyEngine/GameObjects`
     header line plus JSON holding each copied object in the `.prefab` format, its prefab link and world matrix
     (the familiar editor also puts copied objects on the system clipboard). The OS delivers Ctrl+C/X/V as
     copy/cut/paste events rather than key presses, so this is also how the shortcuts are detected; the editor
     keeps its own copy too for the Edit menu. Paste puts the objects right after the active object as its
     siblings (Paste As Child in the row menu), keeps their world placement, gives unique names ("Rig (1)") and
     selects them. Cut = Copy + Delete in one undo step.
150. **Lock View to Selected (Shift+F) keeps the Scene camera's pivot on the selection** every frame (orbit and
     zoom still work); Edit ▸ Lock View to Selected shows a ✔ while on. Edit ▸ Align With View and Grid and
     Snap were added next to Frame Selected.
151. **Ctrl+D no longer duplicates while a text field has focus**, like Undo/Select All, so it can't fire
     while renaming.
152. **Inspector rows no longer overflow narrow Inspectors**: the X/Y/Z rows now budget the axis letters and item
     gaps, and the header's name field leaves room for Static. Before, both were ~20 px wider than the panel, which
     pushed the component ⋮ buttons partly out of view.

153. **Lighting settings belong to the scene** (`Scene::render_settings`, like Unity's per-scene RenderSettings)
     and are saved as an optional top-level `"lighting"` block in the `.scene` file. The block is left out while
     every value is at its default, so scenes saved before (and the generated samples) re-save byte-identical;
     it is parsed on its own, so a malformed block only warns and falls back to the defaults. Prefabs never
     carry it. Undo works through the existing whole-scene snapshots.
154. **The defaults reproduce the old fixed look exactly.** The procedural sky is our own formula built around
     the previous gradient: sky tint mid grey (0.5) is a neutral ×1 factor, atmosphere thickness 1 and exposure 1
     leave the zenith/horizon colours unchanged, and the ground colour default is the old ground colour (Unity's
     defaults are exposure 1.3 and a slightly different ground, but its formula isn't ours). Sun-driven effects
     (warm horizon, darker zenith, night) only start once the sun is below ~25° elevation, so the default 50°
     light changes nothing. Skybox ambient keeps the old 0.45 / 0.28 / 0.3 factors, now applied to the sky colour
     at the zenith, the average of four horizon directions and the nadir, computed on the CPU with a tested
     mirror of `sky_color` in `common.wgsl`.
155. **The sun is the brightest enabled Directional Light** (Unity's behaviour when no Sun Source is set; there is
     no Sun Source picker). The sun disc (None / Simple / High Quality, Unity's 0.04 default size) is drawn only in
     the skybox, not in reflections (the specular highlight already shows the sun), fades below the horizon,
     turns orange at sunset and writes a little to the glow target so it blooms. Disc and halo can change the
     default scene's sky only where the sun is in view, which the default camera doesn't face.
156. **The skybox is configured in the Lighting window instead of a skybox material** (there are no skybox
     materials yet): Procedural (sun disc, sun size, atmosphere thickness, sky tint, ground, exposure) or Solid
     Color. A camera with Solid Color clear flags still clears to its own background, as before.
157. **Environment Reflections have their own intensity multiplier** (source always Skybox), separate from the
     ambient multiplier; before, reflections were scaled by the (fixed) ambient intensity. The Intensity
     Multiplier is shown for every ambient source (Unity shows it only for Skybox). Gradient and Color sources
     use their colours directly as ambient light.
158. **Fog uses the familiar formulas** (Linear start/end, Exponential, Exponential Squared; defaults off, grey,
     Exp², 0.01, 0–300) with radial distance in perspective views and view-plane distance in orthographic ones.
     It applies to lit meshes (emission glow fades too), alpha-blended particles (towards the fog colour) and
     additive particles (towards black, so they don't brighten fog), and hazes a band above the skybox horizon by
     the fog amount at the camera's far distance (Unity's built-in fog leaves the skybox alone; asked for here so
     the horizon blends into the fog).
159. **Lighting is an on-demand tab.** `Tab::ALL` stays "the tabs every built-in layout has" (the
     `every_builtin_layout_has_each_tab_once` test keeps its meaning and now also checks that on-demand tabs are
     absent); `Tab::ON_DEMAND` lists Lighting and `Tab::every()` both (used by `MISTY_TAB`). Window ▸ Rendering ▸
     Lighting docks it next to the Inspector, like Unity's default placement, or focuses it if open. It has Scene
     (informational: everything is realtime, no baking) and Environment pages; the chosen page is kept in egui
     memory, not in the project.
160. **Post-processing is a `Post-process Volume` component** (Add Component ▸ Rendering), global only: the
     enabled, active volume with the highest Priority wins (first in Hierarchy order on ties); there is no
     blending, weight, local/box volume or profile asset. Each effect is an override section with an active
     checkbox (new volumes start with all overrides off, Tonemapping preset to ACES). An inactive override uses
     the engine baseline, which is what every scene had before: Bloom intensity 0.6 / threshold 0 / scatter 0.7,
     no grading, tonemapper None. (In Unity a volume without a Bloom override means no bloom; here emission
     always glows by default, so the baseline keeps it.) No volume = baseline, so existing scenes look identical.
161. **Tonemapping None keeps the old hue-preserving highlight compression** (Unity's None clips per channel).
     Neutral is Hable's filmic curve scaled so mid grey stays ≈0.18 (our own curve, close in spirit to Unity's
     Neutral); ACES is Stephen Hill's fit of the RRT + sRGB ODT (darkens mid grey to ≈0.1, like Unity's ACES).
     Order in the composite: scene + bloom × intensity × tint → vignette → post exposure (EV) → colour filter →
     contrast (log2 space around 0.18) → saturation → tonemap. Steps at their neutral value are skipped so the
     baseline output is bit-for-bit the old one. CPU copies live in `render/post.rs` with tests.
162. **Bloom threshold and scatter adapted to the emission-only bloom source:** threshold applies a soft knee
     (knee = threshold / 2) in the prefilter (0 = everything in the glow buffer blooms, the old behaviour);
     scatter sets the weight of each smaller level in the additive upsample chain (0.3 + scatter, so 0.7 → 1, the
     old behaviour) instead of Unity's lerp between levels. Vignette is centred and non-rounded (Unity's default
     shape) with Unity-like intensity ×3 / smoothness ×5 scaling.
163. **Environment and post values travel in the per-view uniform** (one write per view, read by the sky, mesh,
     particle, bloom and composite shaders); the view-uniform stride for shadow slots grew from 256 to 512 bytes.
164. **The Scene view ☁ toggle is the Effects toggle:** it switches skybox, fog and post-processing together, and
     the ⏷ next to it picks which of the three it controls (Unity's Effects dropdown). With post-processing off
     the Scene view uses the baseline post (the look it always had) rather than raw HDR; the sky still follows
     the scene's sun when Scene lighting is off (headlight).
165. **Lua `RenderSettings`** is a global userdata with `fog`, `fogColor`, `fogDensity`, `fogStartDistance`,
     `fogEndDistance`, `ambientIntensity` and `reflectionIntensity`, writing straight into the running scene's
     settings (Play mode edits are discarded with the rest of the play scene).
166. **The LightingDemo sample is generated by code** (`sample_lighting` test module, ignored writer + check test):
     low evening sun ahead of the camera, hazy warm procedural sky, Exp² fog over a receding row of pillars, and a
     Global Volume with ACES, +0.7 EV, slight contrast/saturation and a vignette. Adds `Sand.mat` and
     `Sandstone.mat`; reuses `Blue Metal.mat` without changing it.

## 2026-11-03 — Physics: queries, layers, joints, Character Controller

167. **`physics.rs` became a `physics/` module** (`mod.rs` world + bodies, `colliders`, `joints`, `character`,
     `queries`, `layers`, `convert` + one test file per feature). With queries, joints and the controller the
     single file would have passed 1500 lines; the existing code moved without behaviour changes (`locked_axes`
     and the collider centre offset became small helpers).
168. **Layer names moved to the engine** (`misty_engine::physics::LAYERS`, re-exported as the Inspector's
     `LAYERS`): scripts need them for `LayerMask.NameToLayer`. The six built-in names and indices are kept (`UI`
     stays 4, matching `ui_create::UI_LAYER`); there is still no Tags & Layers editor for user layers.
169. **Layer Collision Matrix stored as the unchecked pairs** (`physics.ignored_layer_pairs: [[a, b], …]`, smaller
     index first) instead of a 32×32 bit matrix: the JSON stays short and an old settings file means "everything
     collides". It becomes rapier collision groups (membership = the layer bit, filter = the layers it collides
     with), so it applies to contacts, triggers and the Character Controller's sweeps. Changing
     `gameObject.layer` from Lua updates the groups.
170. **Query rules follow the reference engine:** `Physics.Raycast` returns a hit table or `nil` (Lua has no `out`
     parameters); the default mask is `DefaultRaycastLayers` (everything but Ignore Raycast), `OverlapSphere`
     defaults to all layers; rays ignore colliders they start inside; triggers are hit unless the new Project
     Settings ▸ Physics ▸ **Queries Hit Triggers** is off. `RaycastAll` is sorted nearest first (the reference
     leaves the order unspecified). `hit.collider` is the collider's GameObject (there is no Collider userdata);
     GameObject got a `gameObject` field so `hit.collider.gameObject` works.
171. **Queries work before the first step**: building the world (and adding instantiated objects) runs a
     zero-length rapier step, which registers the new bodies and fills the broad phase without moving anything
     (rapier's collision-only pipeline skips the island manager, so dynamic bodies never woke up); contacts found
     then are kept and reported by the next real step, so `OnCollisionEnter` still fires once.
172. **Joints connect connected body → own body** (rapier body 1 = the connected body or a shared fixed "world"
     body, body 2 = the object), with frames taken from the poses when Play starts, so a hinge's angle starts at
     0 and a Fixed Joint keeps the current offset. Hinge limits/spring/motor use rapier's angular X axis of a frame
     whose X is the hinge axis; spring and motor use the force-based motor model (Spring in N·m per radian, motor
     Force = max torque, Target Velocity in °/s); when both are on, the motor wins. Joints require a Rigidbody:
     adding one through Add Component adds a Rigidbody too (`registry::requires`, like RequireComponent).
173. **Spring Joint is a force, not a rapier joint**: the reference's Min/Max Distance band (no force inside it)
     has no rapier equivalent, so before each step `spring × stretch + damper × closing speed` is applied at both
     anchors. With Enable Collision off, an empty rapier joint (no locked axes) is added only to switch contacts
     between the two bodies off. Explicit forces are stable for usual values (default 10/0.2: ω·dt ≈ 0.06).
174. **Character Controller = kinematic rapier body + capsule + `KinematicCharacterController`.** Slope Limit is both
     the climb and the slide angle, Step Offset drives autostep (min tread = radius/2) and ground snapping, Skin
     Width is the controller's offset. `Move` updates the Transform immediately (the body follows in the next
     step); `isGrounded` is rapier's grounded flag after the last move (also true while walking on flat ground
     without gravity, unlike the reference). `SimpleMove` keeps its own falling speed. The capsule is not pushed
     by physics; dynamic bodies can rest on it. A Rigidbody on the same object is ignored with a warning. The
     capsule follows the object's rotation (the reference keeps it upright).
175. **`OnControllerColliderHit` runs right after the callback that called `Move`** (one call per collider per
     move), not inside `Move`: Lua callbacks can't run while the API call holds the script world.
176. **Rigidbody ▸ Interpolate (None/Interpolate)** stores the pose before each step and blends it with the current
     one by the leftover fixed-step time when writing Transforms, so the drawn pose lags up to one step like the
     reference. `rb.position` reads the physics pose. Extrapolate is not offered.
177. **`MovePosition` / `MoveRotation` place the Transform at once** (kinematic bodies then sweep there during the
     next step, pushing what they meet; dynamic ones are placed). `MoveRotation` takes Euler angles because the Lua
     API has no Quaternion type yet.
178. **PhysicsDemo sample generated by code** (`sample_physics` test module, like ParticlesDemo): new scripts
     `CharacterWalker.lua` (tank controls, so the child camera follows) and `LaserPointer.lua`, new `Laser.mat`;
     `PlayerController.lua` and `SampleScene.scene` are untouched. A third test plays the scene headless.
179. **Demo scenes don't share user-edited materials or scenes.** LightingDemo now uses its own `Blue Steel.mat`
     (the original Blue Metal values) instead of `Blue Metal.mat`, which the user edits; the Project rename test checks
     the generated PhysicsDemo/AnimationDemo instead of `SampleScene.scene`. Generated samples and tests stay the
     same whatever the user does to their own assets.


## 2026-11-03 — Animator (state machine)

180. **The Animator is built on the existing clips.** `.anim` files, curve evaluation and child-path binding are
     shared with the legacy-style Animation component (decision 104 left room for this); the new parts are the
     `.controller` asset (`animation::controller`), the play-mode evaluator (`animation::animator`,
     `AnimatorPlayer` in the scripting `World` beside `AnimationPlayer`) and the `Animator` component
     (`scene/animator_component.rs`: Controller, Speed, Keep State On Disable).
181. **`.controller` is readable JSON:** `parameters` (`name`, `type` Float/Int/Bool/Trigger, one numeric
     `default`; bools and triggers are 0/1), `states` (`name`, `motion` clip path or `blend_tree`, `speed`,
     `loop`, graph `position`), `default_state`, a flat `transitions` list (`from`/`to` are `"AnyState"`,
     `"Exit"` or `{"State": name}`) and the graph positions of Entry / Any State / Exit. States and parameters
     are referenced by name (renames follow through transitions, conditions and blend trees). Loading repairs a
     missing default state and drops transitions to missing states.
182. **One base layer, no sub-state machines.** Entry always goes to the default state (no conditional entry
     transitions); a transition to Exit returns to the default state. Layers, avatar masks, sub-state machines,
     state machine behaviours, mirror/foot IK, root motion and transition interruption are not implemented:
     while a cross-fade runs no other transition is checked (the reference's "Interruption Source: None").
183. **Loop is a state setting** (`State.loop`, default on) instead of the clip's Loop Time, so one clip can be
     used looping in one state and once in another without duplicating it. The Animation component keeps using
     the clip's flag.
184. **Transition rules:** Any State transitions are checked first, then the current state's, in list order; the
     first that passes fires and consumes the triggers its conditions use. Any State never re-enters the state
     already playing (Can Transition To Self is always off). A transition with neither an exit time nor a
     condition never fires (the Inspector warns). Exit time ≥ 1 (or a non-looping state) allows leaving once
     normalised time reaches it; exit time < 1 on a looping state allows it from that point to the end of every
     loop. Duration is in seconds (Fixed Duration) or source-state lengths; Offset starts the destination at a
     normalised time.
185. **Cross-fades blend sampled property values linearly** (Euler angles per axis, booleans switch at 0.5).
     The Animator remembers the rest value of every property it animates the first time it writes it ("write
     defaults"): a property missing from one side of a blend fades from/to that rest value, and properties no
     longer animated by the current state go back to it.
186. **Animator and Animation on one object both run**: the Animation component is sampled first, the Animator
     last, so the Animator wins for properties both animate. Animation events in clips played by Animator states
     call script functions as with the Animation component (not for blend-tree children).
187. **"Apply on enable" = Keep State On Disable.** An enabled Animator enters its default state with its default
     parameter values and poses on the first frame; disabling it (or its object) throws that state away unless
     Keep State On Disable is set, so re-enabling restarts it. `animator.speed` multiplies every state's speed.
188. **1D blend trees are included** (a state's `blend_tree`: a Float parameter and children with a clip and a
     threshold; the two neighbours of the value are mixed linearly, clamped at the ends; normalised time is
     shared, its length is the weighted mean of the children's). 2D blend trees and per-child speed/mirror are
     not. They are edited in the Inspector of the state; there is no separate blend tree graph.
189. **Lua `Animator`** (`GetComponent("Animator")`, `scripting/animator_api.rs`): `SetFloat/GetFloat`,
     `SetInteger/GetInteger`, `SetBool/GetBool`, `SetTrigger/ResetTrigger`, `Play(state [, layer, time])` (keeps
     playing when already in that state and no time is given), `CrossFade(state, normalisedDuration)`,
     `CrossFadeInFixedTime(state, seconds)`, `GetCurrentAnimatorStateInfo(0)` / `GetNextAnimatorStateInfo(0)`
     (tables with `normalizedTime`, `length`, `speed`, `loop`, `IsName(name)`; "Base Layer.Name" also matches),
     `IsInTransition(0)`, `speed`, `enabled`. Names are strings (no hashes); a missing parameter / state or a
     type mismatch logs a warning; any layer other than 0 (or -1) is an error.
190. **Animator window = on-demand tab docked with the Scene/Game views** (`Tab::ON_DEMAND`), opened from
     Window ▸ Animation ▸ Animator, the Animator component's Open button, the controller Inspector or by
     double-clicking a `.controller`. It follows the selected object (or its nearest parent) with an Animator and
     keeps the last controller otherwise. Selecting a state or transition clears the object selection and shows
     it in the Inspector (the Inspector edits a `.controller` asset by showing the window's selection).
191. **Controller edits are saved immediately and undo in the normal history:** an undo snapshot's asset is now
     `ClipSnapshot::Clip` or `ClipSnapshot::Controller` (the controller store lives inside `ClipStore`), so
     Ctrl+Z restores graph, parameter and Inspector edits together with the scene. Edits made during one drag or
     one text-field session are one undo step. Edits during Play mode are kept (assets aren't reverted by Stop).
192. **Graph controls:** wheel zooms around the mouse, middle mouse or Alt+left drag pans, left drag moves a
     node, a click on a line selects the transition, Delete removes the selected state/transition, a clip dropped
     from the Project window creates a state playing it. There is no marquee selection, multi-selection,
     copy/paste or "Frame All". Several transitions between the same two states draw as one arrow.
193. **Live view:** in Play mode the window shows the target's current state with a blue outline and progress bar
     (the cross-fade destination dimmer, the transition being played blue) and the live parameter values, which
     can be edited to drive the game. The target is the followed object, else the first object whose Animator
     uses the controller.
194. **AnimatorDemo sample generated by code** (`sample_animator` test module: ignored writer + check test that
     also plays it headless): `Hero.controller` with a Locomotion blend tree (`HeroIdle` / `HeroWalk`), Jump from
     Any State on a trigger (back through Exit at exit time 0.9) and Dance on a bool; `AnimatorControl.lua`
     (Space, 1/2/3, 4) writes the state into a UI Text under the key hint.


## 2026-11-03 — Terrain

195. **Terrain data is its own asset, `.terrain`**: JSON with the resolutions, size and layers readable, and the two
     big arrays as base64 — heights as 16-bit little-endian integers (0..65535 = 0..1 × Terrain Height, the same
     precision as the reference's heightmap) and the splat map as RGBA bytes (one channel per layer). A 257² map
     is ≈180 KB; JSON stays diffable for everything but the arrays. Base64 is a ~40-line codec in `terrain/file.rs`
     (no new dependency). Wrong array lengths or resolutions are padded/snapped on load.
196. **Terrain layers live inside the terrain data** (name, diffuse, optional normal map + strength, tile size and
     offset in metres, metallic, smoothness) instead of separate layer assets; at most four, matching one RGBA
     splat texture. Removing a layer moves its weight to the others.
197. **Smaller defaults than the reference**: a new terrain is 200 × 60 × 200 m with a 257² heightmap and a 256²
     control texture (the reference uses 1000 × 600 × 1000, 513², 512²). The directional shadow map is fitted to all
     casters, so a kilometre-wide terrain would make shadows very soft; resolutions are limited to 33–1025 (heights)
     and 16–512 (splat). New terrains are centred on the Scene view pivot at y = 0, with a Terrain Collider, and
     their data goes to `Assets/New Terrain.terrain` (numbered when taken).
198. **One shared, in-memory copy per asset** (`terrain::store`, keyed by absolute path): the renderer, physics,
     scripts and brushes all read it; every change bumps a version so the renderer rebuilds that terrain's mesh and
     splat texture only when needed. Brush strokes edit the store; the file is written when the stroke (or a
     Settings drag) ends, like material edits. Edits made in Play mode are not reverted (terrain data is an asset).
     Brushes only work in Edit mode.
199. **Heights, mesh and collider share one triangulation**: cell diagonals run from (x0, z1) to (x1, z0), the split
     rapier's heightfield uses, and `SampleHeight` interpolates over that triangle, so sampled heights equal the
     collider surface exactly. The mesh has one vertex per sample (no LOD/chunks), smooth central-difference
     normals and tangents along +x for layer normal maps.
200. **Terrain uses only its object's position** (rotation and scale are ignored, as in the reference) for drawing,
     the collider, `SampleHeight` and picking. The Terrain Collider is always static and placed in world space;
     its Terrain Data can be left empty to use the Terrain's. `SampleHeight` returns the height relative to the
     terrain's position.
201. **Terrain rendering reuses the lit mesh shader**: `mesh.wgsl`'s fragment lighting moved into `shade(world,
     Surface)` (no behaviour change), and `terrain.wgsl` is appended to it with an `fs_terrain` entry that blends the
     layers by the splat map (weights past the layer count ignored, then normalised; no layers = grey) and calls
     `shade`, so terrains get the same lights, shadows (cast and received), ambient, reflections and fog. Terrain
     textures bind at group 3; group 2 gets the default material group. Terrains use draw slots after the meshes
     and are included when fitting the directional shadow map. No selection outline (like the reference).
202. **Brush model**: round brush with Size (diameter, metres), Opacity and Falloff (0 = hard edge, 1 = fades from
     the centre; smoothstep). Rates at full opacity: Raise/Lower 15 % of Terrain Height per second (Shift lowers),
     Set Height and Smooth approach their target at 8/s (Smooth uses the 3×3 average), Paint grows the layer's
     weight at 4/s while the others shrink so the bytes always sum to 255. "Flatten All" sets every height to the
     Set Height target. One undo step per stroke: the data is copied when the stroke starts and the undo entry
     carries it (`Snapshot::terrain`; undo/redo swap it back into the store and save the file).
203. **While a terrain is selected and a brush tab is active, the transform handles hide** and clicks over the
     terrain paint instead of picking; the brush circle (rim + inner falloff ring) is drawn as gizmo lines that
     follow the surface. Picking and surface snapping hit terrains by ray-marching the heightmap.
204. **Lua: `Terrain.activeTerrain`** is the first enabled terrain in Hierarchy order (`Terrain.activeTerrains`
     lists all); `GetComponent("Terrain")` works too. A terrain has `SampleHeight(position)`, `GetPosition()`,
     `terrainData` (a table: `size`, `heightmapResolution`, `alphamapResolution`, `layerCount`), `gameObject`,
     `transform`.
205. **TerrainDemo sample generated by code** (`sample_terrain` test module: ignored writer, a check test and a
     headless play test): 120 m terrain with a hill, ridge and shore, splat painted from height and slope (sand
     low, rock steep, grass elsewhere), four generated tileable 256² textures (grass, rock + rock normal map,
     sand), balls and crates that roll down when playing, and `TerrainHover.lua` circling the hill with
     `SampleHeight`. Reuses `Red.mat`, `Wood.mat` and `Laser.mat` unchanged.


## 2026-11-03 — uGUI widgets, auto layout and the event system

206. **Widget parts are referenced, not found by name.** Toggle (Graphic, Group), Slider (Fill / Handle Rect),
     Scrollbar (Handle Rect), Input Field (Text Component, Placeholder), Dropdown (Template, Caption / Item Text),
     Scroll Rect (Content, Viewport, scrollbars), Event System (First Selected) and every widget's Target Graphic
     are object references, remapped in scene files and prefabs like On Click targets (the reference table also
     reaches into the nested `selectable` block). `Scene::copy_subtree` (Duplicate, copy/paste, `Instantiate`)
     now points references between objects of the copied subtree at the copies, so a duplicated Slider drives
     its own Fill; references to objects outside the subtree are kept.
207. **Driven values are computed, not written.** Slider Fill/Handle and Scrollbar Handle anchors, the Toggle
     checkmark, the Input Field's displayed text / Placeholder visibility, the Dropdown caption and layout-group
     child rects are derived from the widget values every frame in the layout and draw passes (edit mode and
     play mode alike) instead of being stored into the parts' components. The Inspector shows "Some values driven
     by …" on such Rect Transforms but keeps the fields editable (their stored values are simply unused).
208. **Layout follows the familiar algorithm** (our own implementation): min / preferred / flexible sizes per
     axis from Layout Element (each set value overrides), layout groups (computed from their children) and Text
     (preferred = text extent, wrapped at the width it gets); Image has no native size (no sprites), so it reports
     0. Linear groups lerp between min and preferred, then share spare space by flexible size; Control Child Size
     off keeps the child's Size Delta and aligns it in its cell; Child Force Expand gives flexible ≥ 1. Widths are
     resolved before heights so wrapped text gets the right height. Grid Layout Group always sizes children to
     the cell. Content Size Fitter resizes around the pivot (keeps its Rect Transform values, like the rest).
     Layout is recomputed every frame (no dirty flags).
209. **Masks clip on the CPU.** Mask and Rect Mask 2D clip descendants to their screen rect (Rect Mask 2D with
     padding); quads are cut and their UVs shrunk in the draw list, raycasts outside the clip miss. UI graphics
     are axis-aligned rectangles (no sprites, no rotation), so this is exact without a stencil buffer; a Mask's
     shape is therefore always its rect. Show Mask Graphic off hides the Mask's own Image.
210. **One event system, always on.** Pointer/keyboard handling runs while playing whether or not the scene has an
     Event System object (UIDemo has none and keeps working); the object only holds settings (First Selected,
     Send Navigation Events, Drag Threshold 10 px) and has no separate input-module component. GameObject ▸ UI
     creates an `EventSystem` root object with the first Canvas, like the reference editor. The UIDemo generator
     removes it so the committed UIDemo.scene stays byte-identical.
211. **Pointer routing:** the press goes to the nearest selectable ancestor of the topmost raycast hit, drags to
     the nearest Slider / Scrollbar / Input Field / Scroll Rect. Sliders and scrollbars drag at once; a Scroll Rect
     starts after the drag threshold, then cancels the press (no click), measuring from the press point (the
     reference measures from where the threshold was crossed). Pressing empty space deselects. Colour Tint order:
     Disabled > Pressed > Selected > Highlighted > Normal; colours switch instantly (no fade duration). Button
     got a Selected Color (left out of files at its default so older scenes re-save unchanged).
212. **Value-changed events come from comparing values each frame** (Toggle isOn, Slider / Scrollbar value, Input
     Field text, Dropdown value), so changes made by scripts or toggle groups are reported too — in the next UI
     step instead of inside the assignment. Objects created this frame (instantiated, dropdown items) report from
     the next one. Persistent calls (Inspector lists) get the value as the method argument; Lua listeners are
     added with `AddListener` on `onClick` / `onValueChanged` / `onEndEdit` and stored per object in a Lua registry
     table.
213. **Toggle Group:** a toggle switched on (by a click or a script) turns the others of its group off; with Allow
     Switch Off unset, clicking the toggle that is on does nothing, and a group with none on gets its first
     toggle switched on when Play starts.
214. **Input Field** is single line only (no multi-line / submit modes, no rich text, no clipboard shortcuts besides
     what the OS turns into typed text, e.g. egui paste). Click places the caret, drag selects, Shift+arrows /
     Home / End / Ctrl+A select, Backspace/Delete, Return ends editing (field stays selected), Escape restores the
     text from before editing, clicking elsewhere ends editing. Content types Standard, Integer Number, Decimal
     Number (one '.', leading '-') and Password (shown as '*'); Character Limit counts characters. Long text scrolls
     so the caret stays visible and is clipped to the Text rect. Script/Inspector text is sanitised the same way.
     Typed text and repeating key presses reach the engine through new `Input.text` / `Input.typed`.
215. **Dropdown lists are real objects:** opening copies the inactive Template under the root canvas as "Dropdown
     List" (last child, so it draws on top of that canvas), one copy of the template's Item per option, shrunk
     when the options don't fill it and opened above the dropdown when there's no room below. Any press outside
     the list closes it and is swallowed (the reference uses a full-screen blocker object). There is no fade, no
     scroll to the selected item and no per-item images.
216. **Scroll Rect:** Unrestricted / Elastic (our own critically damped spring, Elasticity ≈ settle time, rubber
     band while dragging) / Clamped, inertia with Deceleration Rate per second, wheel scrolling in pixels × Scroll
     Sensitivity (default 1; one wheel notch ≈ 50 px in the player, egui's delta in the editor). Linked scrollbars
     get value and size every frame and move the content when dragged or clicked (a click pages by the handle
     size). No visibility modes (scrollbars are always shown) and no onValueChanged for the Scroll Rect itself.
217. **Navigation is spatial and arrow-key only:** with Send Navigation Events on, arrows move the selection to the
     best selectable in that direction (score = dot / distance²), or step a selected Slider by 10 % (1 with whole
     numbers); Return / Space submit (click a Button, flip a Toggle, open a Dropdown, start editing an Input
     Field). No explicit navigation modes and no Tab.
218. **Default widget hierarchies live in the engine** (`ui::presets`), so the editor menu, samples and tests build
     the same objects. Without sprite assets the parts are plain coloured rectangles (light grey track, blue-ish
     Fill, white handle, dark square checkmark, small dark "arrow" bar) with the reference layouts' sizes and
     offsets. GameObject ▸ UI lists Text, Image, Raw Image | Button, Toggle, Slider, Scrollbar, Dropdown, Input
     Field | Canvas, Panel, Scroll View | Event System.
219. **UIWidgetsDemo is generated by code** (`sample_ui_widgets` test module, ignored writer + check + play test):
     a settings panel laid out with Vertical/Horizontal Layout Groups and Layout Elements, a scroll list of level
     buttons (Vertical Layout Group + Content Size Fitter), and `SettingsMenu.lua` updating a summary Text from
     listeners plus an On End Edit call. Its Canvas Scaler uses a 1280×720 reference matched to height so the
     panel fits any window.
220. **Rendering modes are pipeline variants.** `RenderingMode` (Opaque / Cutout / Fade / Transparent) plus
     Unlit and Double Sided pick one mesh pipeline (`render/variants.rs`), built the first time a frame needs it.
     Cutout discards in its own entry point (`fs_cutout`) so Opaque keeps early depth. Fade = straight alpha
     blending (specular fades too); Transparent = premultiplied: the diffuse is scaled by alpha and the output
     alpha is `1 − oneMinusReflectivity + alpha × oneMinusReflectivity` (0.96 for dielectrics), so reflections
     and highlights stay, like the reference. Blended modes test but don't write depth; their emission blends
     into the bloom target the same way.
221. **Draw order:** Opaque (queue 2000) then Cutout (2450) front to back, terrains, the grid, then Fade /
     Transparent (3000) back to front, then particles, lines and the outline. Distance is to the object's origin
     (squared distance, or along the view axis when orthographic); ties keep Hierarchy order. Transparent meshes
     and particles are sorted separately (all meshes before all particles).
222. **Shadows per material:** Opaque casts as before; Cutout casts clipped shadows (the shadow pass samples the
     albedo alpha, `fs_shadow_cutout`); Fade, Transparent and the Unlit shaders cast none (the reference dithers
     semi-transparent casters; its unlit built-ins have no caster pass). Double Sided casters render both faces.
     Receiving is unchanged (Unlit ignores light and shadows).
223. **Maps:** Metallic map R = metallic and A = smoothness; with a map the Metallic slider is hidden (metallic
     = R) and Smoothness scales A — one value stands for both Glossiness and Gloss Map Scale. Smoothness Source =
     Albedo Alpha takes smoothness from the albedo alpha and then alpha comes from the colour only. Occlusion =
     G channel, `lerp(1, G, strength)`, applied to ambient light and reflections only. Emission map (sRGB) ×
     emission colour × intensity, only while Emission is on. All main maps share one Tiling/Offset. Data maps
     (metallic, occlusion) are sampled as linear textures; unset maps bind a 1×1 white texture, so older
     materials shade exactly as before.
224. **Double Sided is an extra Advanced Option** (the reference Standard shader has no culling toggle): Cull Off
     in the main and shadow passes, back faces lit with the flipped normal.
225. **Unlit shaders:** `Unlit/Color` = colour only, `Unlit/Texture` = texture only, `Unlit/Transparent` = texture
     with alpha blending (Fade queue); all are fogged, unlit, cast/receive no shadows and add no glow. The shader
     is stored by name in the `.mat` (unknown names fall back to Standard) and switching keeps every value.
226. **Materials list:** Mesh Renderer `material` stays Element 0; `extra_materials` holds elements 1.. (left out
     of files when empty). Our meshes have one submesh, so each extra element draws the mesh again on top, like
     the reference does for extra materials on the last submesh. The Inspector has a Size field (1–8).
227. **Instance and shared materials (Play mode only).** `renderer.material` copies the element's material on
     first use into `MeshRenderer.instances` (never saved, gone when Play stops) — only that object changes.
     `renderer.sharedMaterial` edits go to `Scene.shared_materials` by asset path and show on every renderer
     using that asset; they are not written to the `.mat` (the reference editor writes them to the asset).
     Renderers resolve instance → script-changed asset → asset → Default-Material. Default-Material can't be
     changed from scripts.
228. **Lua material properties use the Standard names** (`_Color`, `_EmissionColor`, `_Metallic`, `_Glossiness`,
     `_GlossMapScale`, `_BumpScale`, `_Cutoff`, `_OcclusionStrength`, `_Mode`, `_SmoothnessTextureChannel`,
     `_Cull`, `_MainTex`, `_BumpMap`, `_MetallicGlossMap`, `_OcclusionMap`, `_EmissionMap`). `_EmissionColor` is
     colour × intensity; setting it divides by the brightest component above 1. `SetFloat("_Mode")` switches the
     mode completely (no keywords or render queue to set by hand); only `_EMISSION` is a switchable keyword.
     Unknown names are ignored. Texture offset/scale use Vector3 (z ignored) since Lua has no Vector2 yet.
229. **Material previews** are rendered by the engine renderer after the UI pass (`material_preview`), so a new
     preview appears one frame later: two directional lights, a dark solid background, the default lighting
     environment for reflections. Project thumbnails are 128 px, rendered into one shared target and copied to
     their own texture, at most 6 per frame, and re-rendered when the `.mat` file's time changes. The Inspector
     preview (bottom of the material Inspector, sphere or cube, drag to rotate) re-renders when the material,
     size or rotation changes. Only Project-selected materials get the preview, not the inline editors.
230. **`.mat` compatibility:** fields added for this step are left out of the file at their defaults, so the
     committed materials re-save byte-identical (tested); old files load as Opaque, cutoff 0.5, strength 1.
231. **MaterialsDemo is generated by code** (`sample_materials`, ignored writer + check + play test) with its own
     generated textures (fence lattice with alpha, hazard sign, panel glow grid, metallic stripes) and materials;
     the floor reuses `Checker.mat`. `MISTY_ASSET=<Assets/…>` selects a Project asset for editor screenshots.


## 2026-11-03 — Audio Mixer

232. **Mixer groups are kira sub-tracks.** Each group of a loaded mixer becomes a track under its parent's track
     (Master under kira's main track) with the group's effect chain and a peak-meter effect at the end; voices play
     on their Output group's track. Volume and effect values are pushed to the tracks every frame. kira fixes a
     track's effect chain when it is built, so adding / removing / reordering groups or effects while playing
     rebuilds that mixer's tracks and restarts the sources routed to it.
233. **Effects map onto kira's built-in effects:** Lowpass / Highpass = kira Filter (resonance Q 1..10 mapped to
     kira's 0..0.9), Echo = Delay (decay % → feedback dB, wet / (dry + wet) → mix; the delay time is read when the
     track is built), Reverb = Reverb (room size → feedback, damping, width, wet mix %), Distortion = soft clip
     (Level 0..1 → 0..36 dB drive), Compressor = Compressor (threshold, ratio, attack, release, make-up gain). Bypass
     = the effect's mix at fully dry. Effect parameters are described by one table per kind (name, unit, range,
     default) shared by the backend, snapshots, the Inspector and exposure.
234. **Mixer values are addressed by id keys** (`"<group>/Volume"`, `"<group>/Pitch"`, `"<group>/<effect>/<param>"`).
     Group and effect ids never change, so renames keep snapshots and exposed parameters valid; snapshots store only
     the values that differ from the defaults, and deleting a group or effect prunes its keys. Group names are kept
     unique so outputs can name them.
235. **Snapshots hold volumes, pitches and effect values; Mute / Solo / Bypass are per mixer** (not per snapshot), like
     the reference. `TransitionTo(t)` blends every value linearly (in dB for volumes) from where it is now; faders in
     the window edit the snapshot selected there (in Play mode: the game's current snapshot).
236. **Group pitch and Solo are applied per voice, not per track:** kira tracks have no pitch, so a voice's playback
     rate is multiplied by the pitches from its group up to Master. With any group soloed, voices routed outside
     every soloed subtree are silenced (that includes sounds routed to Master itself). Mute sets the track to
     silence.
237. **No Sends / Receives yet** (the task called them optional): kira send tracks must be wired when a track is built
     and don't nest under sub-tracks, so a faithful Send → Receive would need a parallel track tree. Recorded as a
     gap.
238. **Audio Source Output is a string `"<mixer path>#<group name>"`**, like model sub-asset references; no `#` means
     the mixer's Master. A missing mixer or group warns once and the source plays straight to the listener. New
     Audio Source fields (output, priority, bypass flags, reverb zone mix, Doppler level, spread) are left out of
     files at their defaults so older scenes re-save byte-identically.
239. **`AudioMixer.Find(name)` searches by file name, not a Resources folder:** a loaded mixer with that name, else the
     first `.mixer` under `Assets/` (sorted path order) whose file stem matches. Mixers load on first use (an
     Output, `Find`, or a group lookup) and stay loaded for the rest of Play. `outputAudioMixerGroup = "SFX"` looks
     in the current Output's mixer first, then loaded mixers, then all project mixers.
240. **Per-source filters and reverb zones get the voice its own track.** A source with Low Pass / High Pass / Echo
     Filter components (same object) or in a scene with an Audio Reverb Zone plays on a private track (under its
     Output group) holding those effects; it is removed with the voice. Bypass Effects bypasses the filters, Bypass
     Reverb Zones the zone reverb. Zones: weight 1 inside Min Distance fading to 0 at Max Distance around the
     *listener*; the strongest zone wins (no blending); wet = preset wet × weight × Reverb Zone Mix. Cheap with kira
     (an extra track per voice) and exact for the few voices a scene has.
241. **Doppler, Spread and Priority are our own math.** Doppler: `(c + v_listener·d) / (c + v_source·d)` with
     c = 343 m/s and velocities from last frame's positions (scaled delta time), blended by Doppler Level ×
     Spatial Blend and clamped to 0.5..2. Spread multiplies the 3D pan by `cos(spread / 2)`. Beyond 32 live voices
     the ones with the highest Priority value (then the quietest) are silenced, like virtual voices.
242. **`PlayClipAtPoint` is a voice, not a GameObject:** a fully 3D one-shot at a fixed point with the default
     rolloff, straight to the listener (the reference spawns a temporary object; nothing scripts could see differs).
     `source.time` reads / seeks the `Play()` voice (kira `position` / `seek_to`).
243. **VU meters read a peak-meter effect** appended to every track: the audio thread keeps the loudest sample since
     the last read in an atomic, the Mixer window reads it each frame and lets the bar fall 30 dB/s. The null backend
     estimates a level per track from the voices routed through it (voice volume × the track volumes in between), so
     tests can check routing and mute without a device.
244. **Fader scale is piecewise linear in dB** with more travel near 0 dB (-80, -40, -20, -10, 0, +10, +20 at 0, 22, 42,
     58, 78, 90, 100 %), similar in feel to the reference fader.
245. **Mixer edits in the editor go through a `MixerStore`** (beside Animator Controllers in the clip store, so undo
     snapshots can restore them): every edit saves the file and bumps a generation counter; in Play mode the Mixer
     window sends the edited asset to the running game when the counter changes (`Audio::replace_mixer`), which
     keeps the current snapshot and drops a running transition.
246. **AudioDemo is generated by code** (`sample_audio` test module: ignored writer + check + headless play test). Its
     clips are synthesized (a 4 s plucked arpeggio over a bass, a 1 s humming tone, a short blip) with whole cycles
     so the loops are seamless; the mixer is Master (Lowpass) ▸ Music, SFX (Reverb) with Normal / Muffled / Cave
     snapshots and MusicVol / SFXVol exposed. Like Beep.wav the check compares WAV sizes and headers only.
247. **Screenshot switches:** `MISTY_SCENE=<path>` opens a scene and `MISTY_PLAY=1` enters Play mode before the
     capture, so live views (meters, Game view UI) can be checked.



## 2026-11-03 — Navigation (nav mesh, agents)

248. **Our own nav mesh bake, no crate.** The Rust options were Recast bindings (C++ build, unclear maintenance),
     engine-bound crates (Bevy) or path followers that need a mesh as input, so `navigation` implements a small
     voxel bake in the same spirit: rasterise static triangles into span columns (cell = radius / 3 unless Manual
     Voxel Size), mark tops walkable by slope, merge overlapping spans (flags merge within the step height), make
     kerbs up to a step walkable, keep tops with Agent Height of clearance, link neighbours within the step height,
     erode by the radius (2/3 chamfer distance, like the usual bakers), drop islands under Min Region Area. Closed
     primitives (cube, sphere, capsule, cylinder) are rasterised as solids (lowest to highest point per column)
     so nothing walkable appears inside hollow boxes; planes, quads and imported meshes are surfaces. Huge levels
     get coarser voxels (at most 4 M columns).
249. **Polygons are planar rectangles of cells**, grown greedily in scan order (each seed tries its neighbours'
     slope and flat, rows along x and along z, the biggest wins), then neighbours sharing a whole edge on the
     same plane are joined. Edges are grid-aligned (stair-stepped diagonals) and there are T-junctions; links
     between polygons store the shared edge segment (the portal) instead of shared vertices. Vertex heights come
     from the rectangle's plane, so ramps are exact and terrains are approximated within 8 cm. No detail mesh.
250. **Asset and settings.** The bake writes `<scene folder>/<Scene>/NavMesh.navmesh` (JSON: a header with the
     agent and voxel size, then one polygon per line with its vertices and links). The scene file gets a
     `"navigation"` block (agent radius/height/step/slope, advanced settings, the asset path), left out at its
     defaults so older scenes save unchanged; agent settings are per scene (no project-wide agent types, only
     "Humanoid"). Baking an unsaved scene asks to save it first. Bakes run on a worker thread with a progress
     line; Clear moves the asset to the Recycle Bin. Loaded meshes are cached by path + modification time.
251. **Static = Navigation Static.** Objects take part when they're active and have the GameObject Static flag
     with an enabled Mesh Renderer + mesh or a Terrain; the Navigation window's Object tab edits the same flag.
     No areas/costs (everything is Walkable), no Nav Mesh Modifier, no off-mesh links, no NavMeshSurface
     component (a legacy-style per-scene bake from Window ▸ AI ▸ Navigation).
252. **Paths:** A* over polygons through portal midpoints (the last step goes straight to the target), then the
     simple stupid funnel on x/z with the portal points' heights. Start/end snap to the nearest mesh point within
     the agent height (or 4 cells); unreachable targets give a partial path to the explored polygon closest to
     the target. Paths are computed at once (`pathPending` is always false).
253. **Agents** update after `Update` (before animations and LateUpdate): drop reached corners (10 cm), steer to
     the next corner (with Auto Braking the speed is limited to √(2·a·d) before the end), accelerate towards the
     desired velocity by Acceleration, never overshoot the destination, move along the mesh (the polygon under
     the target among the nearby connected ones, else the closest point on them = sliding along edges) and
     follow its height; the Transform is the mesh position + Base Offset, turned (yaw only) at Angular Speed.
     Arriving with Stopping Distance 0 snaps to the destination and drops the path; a stopping distance keeps
     the path (`remainingDistance` ≤ it). A script moving the Transform warps the agent there. Agents farther
     than the agent height from the mesh at start log "not close enough to the NavMesh" and don't move.
254. **Avoidance is separation, not velocity obstacles.** Every quality except None steers away from agents
     within 2× the radius sum (1.1× while standing at the goal) and from obstacles, with a sideways part for
     things ahead so head-on meetings pass; afterwards overlaps are pushed apart. The agent with the lower
     Priority number moves less (0.2/0.8 split, equal priorities 0.5/0.5).
255. **Nav Mesh Obstacle doesn't carve.** Box or capsule footprint (scaled, yaw from the object); agents steer
     around it and are pushed out of it, paths don't change (Carve is shown disabled).
256. **Scene view overlay:** while the Navigation window is shown (and Show NavMesh is on) the baked mesh is drawn
     as translucent blue triangles 3 cm above the surface, with lighter polygon edges, through a new depth-tested,
     non-writing overlay pass that shares the gizmo line buffer (`Frame::overlay`). Selected agents show their
     cylinder (radius/height from the surface), obstacles their shape, and in Play mode the selected agents'
     remaining path in yellow.
257. **Lua:** `GetComponent("NavMeshAgent")` with `SetDestination`, `destination` (the snapped end; setting it
     calls SetDestination), `remainingDistance`, `hasPath`, `pathPending`, `pathStatus`, `isOnNavMesh`,
     `isStopped`, `velocity` (settable), `nextPosition`, `steeringTarget`, the steering settings, `ResetPath`,
     `Warp`; `NavMesh.SamplePosition(pos, maxDistance)` → `{position, distance, hit}` or nil;
     `NavMesh.CalculatePath(a, b)` → list of corners with a `status` field. For click-to-move, `Camera.main`
     (first enabled camera tagged MainCamera) with `ScreenPointToRay` (→ `{origin, direction}`) and
     `WorldToScreenPoint`, and `Input.mousePosition` (pixels from the bottom-left) were added.
258. **NavigationDemo is generated by code** (`sample_navigation`, ignored writer + check + headless play test):
     a walled yard with maze walls, a 1.5 m platform with a 20° ramp, a stepped terrace and crates; two patrols
     (`Patrol.lua`) and three chasers (`Chaser.lua`) following a marker that clicks move (`ClickMarker.lua`).
     Reuses the Checker, Sandstone, Wood, Bumpy Tiles, Blue Steel, Red and Laser materials unchanged. The editor
     capture also accepts `MISTY_PLAY=1` to screenshot Play mode.

## 2026-11-03 — Code Editor

259. **Custom text widget over a line buffer, not `TextEdit`.** egui's `TextEdit` with a layouter can colour text,
     but multi-caret editing, box selection, squiggles, folding, line commands and VS-style undo need control of
     the model, so the Code Editor has its own: `Vec<String>` lines with character columns (`text`), selections
     (`editor`), commands (`commands`), find (`find`), Lua tokens (`lexer`), completion and diagnostics — all
     pure and unit-tested — and an egui view that lays out each visible line as a coloured galley. Scripts are
     small, so whole-line strings are fast enough without a rope.
260. **Undo = whole-text snapshots per file, grouped by typing bursts.** An edit records the text and carets
     before it unless it continues the open burst: same kind (typing or erasing) within 1.2 s and no caret move,
     Enter or other command in between. Every change gets a new version number; undo restores the version too,
     so undoing back to the saved text clears the dirty marker (●).
261. **Multi-caret editing everywhere.** Each command is a list of edits applied bottom-up; every selection (and
     every fold marker) is moved by each edit, so typing, Backspace, paste, comments, Ctrl+D and line moves work
     with several carets. Box selection (Alt+Shift+arrows / drag) is one selection per line. Alt+click adds a
     caret, Shift+Alt+. / ; add the next / all occurrences, Ctrl+Alt+Up/Down add a caret above/below.
262. **Visual Studio keys where they don't clash with the task:** Ctrl+L cuts the line and Ctrl+Shift+L deletes
     it; Ctrl+] goes to the matching bracket (indent stays on Tab / Shift+Tab); Ctrl+E, W toggles word wrap;
     Ctrl+= / Ctrl+- zoom (VS uses Ctrl+- for Navigate Backward, which isn't implemented); F1 lists the
     shortcuts. While the text has focus it owns the keyboard: editor-wide shortcuts are skipped except Play,
     Pause and Step, and egui's own Ctrl+= UI zoom is turned off. F5 / Shift+F5 enter / stop Play mode from
     anywhere but a text field.
263. **Clipboard like Visual Studio:** Copy/Cut with nothing selected takes whole lines; pasting exactly that text
     again inserts it above the caret's line. With several carets and as many pasted lines, each caret gets one.
264. **Syntax check = compile without running** (`misty_engine::scripting::syntax::check`: `Lua::load(..)
     .into_function()` with the script's path as chunk name), 400 ms after typing stops. The error's line comes
     from Lua's message; the squiggle covers the token after `near '…'`, the last character for `<eof>`, else
     the line. Only the first error is shown (Lua stops there).
265. **Console messages that name `Assets/….lua:N` get a link** (`Rotator.lua:12`); clicking it or
     double-clicking the row opens the script at that line in the Code Editor, even when Preferences choose an
     external editor (it can't jump to a line).
266. **Script editor preference:** Edit ▸ Preferences ▸ External Tools ▸ Script Editor picks the Misty Code
     Editor (default) or the system's default app; both the file tab menu and the Inspector's ⋮ menu keep an
     "Open in External Editor" entry. Preferences and Code Editor options (font size, word wrap, minimap, line
     numbers, auto-close, tab size) are per user in `Library/CodeEditor.json`.
267. **Closing the Code Editor tab keeps the open files** (with unsaved edits) in memory, like hiding a tool
     window; they come back when it reopens. Closing a file (×, middle-click, Ctrl+F4) and quitting the editor
     ask Save / Don't Save / Cancel for unsaved scripts first; the quit prompt comes before the scene's.
268. **Files changed on disk** are checked twice a second: unmodified ones reload (as an undoable step),
     modified ones show a "changed outside the editor" bar with Reload / Keep Mine. Renaming or moving a script
     in the Project window renames its open tab. A new script from the Project window opens in the Code Editor
     without taking the keyboard from the name field.
269. **Saving follows the existing hot reload** (decision 51): in Edit mode the asset database sees the new file
     and re-reads fields (errors go to the Console); in Play mode the running game keeps the version it started
     with and the status bar says the change applies when Play mode ends. Editing never blocks Play mode.
270. **Folding by block** (`function`/`if`/`for`/`while`/`do`/`repeat` … `end`/`until`, multi-line `{ }` and
     `( )`) from the lexer's block structure, which also drives auto-indent, auto-dedent of `end`/`else`/`until`/
     `}` and Format Document (Ctrl+K, D re-indents by depth with the tab size and strips trailing spaces; long
     strings are left alone). Folds are markers that move with edits; a caret moving into a folded block opens
     it. In word-wrap mode Up/Down move by whole lines, not by wrapped rows.
271. **Autocomplete data is a static table** (`code_editor/api.rs`, written from `docs/scripting.md`) of globals,
     their members and object types (`#Transform`, `#Rigidbody`, …) with signatures and one-line docs. Member
     completion resolves `self`, `.transform`, `.gameObject`, the script's own class table, variables assigned
     from `GetComponent("Type")`, `Physics.Raycast` hits and vectors; unknown objects list every object member.
     Ranking: exact case prefix, then case-insensitive prefix, camel humps (`gkd` → GetKeyDown), substring; then
     API before keywords before names from the file, shorter first. Enter on a fully typed name inserts a new
     line instead of accepting. Hovering an API name shows its signature (quick info); Ctrl+click and F12 go to
     a definition in the same file.

272. **Our own profiler, not puffin or tracing.** `puffin` comes with an egui viewer tied to
     particular egui versions and its own frame/scope model; `tracing` needs a subscriber and
     per-event allocation to get timings out. A small `profiler` module is enough: the
     `profile_scope!(name[, Category])` macro opens a guard that records a sample (name,
     category, depth, start, duration) into a thread-local list; while recording is off it
     costs one relaxed atomic load and the name expression (which may be a `format!`, e.g. the
     per-script "Orbit.Update()") is never evaluated. `end_frame()` on the main loop's thread
     moves the samples, the Rendering/Memory counters and the wall-clock frame time into a
     300-frame history. Children inherit the parent's category; the CPU chart stacks each
     category's *self* time, and Others also gets the time no scope covers (editor UI, vsync).
273. **Profiler window** (Window ▸ Analysis ▸ Profiler, Ctrl+7, on-demand tab docking with the
     Project/Console, so the Game view keeps rendering): Record / Clear / previous / next / Current; module charts with the
     newest frame on the right: CPU Usage (stacked categories, legend toggles, 60/30/15 FPS
     lines, GPU time as a yellow line), Rendering (batches) and Memory (GPU MB). Clicking a
     chart selects a frame and its module; details show the merged Hierarchy (Total %, Self %,
     Calls, Time ms, Self ms, expandable by path) or a Timeline of the samples, or the
     Rendering / Memory counters. Recording works in Edit and Play mode (one profiler frame per
     editor frame); Rendering/Memory counters describe the Game view. No Deep Profile, no
     per-thread rows, no saving/loading captures in the editor, no GC/allocation counters.
274. **GPU time** comes from pass timestamp queries: the device is created with
     `TIMESTAMP_QUERY` when the adapter has it (editor through egui-wgpu's device descriptor,
     the player and headless tests directly). The first pass of a render (now the selection
     mask) writes the start, the final composite the end; up to 8 renders are read back
     asynchronously and reported to the frame they were submitted in. Without the feature the
     GPU line and value are simply missing. egui's own rendering is not included.
275. **Player capture:** `MISTY_PROFILE=<file>` makes the player record from the first frame and
     write one JSON object per frame (JSON Lines, 4 frames late so GPU times are in) until it
     quits. There is no viewer for captures yet.
276. **Culling** is done in the renderer, per render: each mesh's local box (primitive half
     extents, imported mesh bounds) is transformed to a world AABB and tested against the six
     planes of the camera's view-projection; terrains are culled whole (bounds cached per
     terrain version), particle systems by the box around their live particles. Every shadow
     map (directional, each spot, each point-light face) culls its casters against its own
     matrix. The test is conservative, so it only drops what the GPU would have clipped anyway;
     the directional map is still fitted around *all* casters (no cascades), so it culls
     nothing there. No occlusion culling, no layer cull masks or per-layer cull distances, no
     LOD groups.
277. **Automatic GPU instancing instead of static batching.** Per-draw data already lives in a
     storage buffer, so instancing needs no new vertex streams: a second storage buffer maps
     `instance_index → draw slot` and the shaders read `draws[slots[instance_index]]` (model
     and normal matrix, colour, material values). Draws with the same mesh, pipeline variant
     and material texture bind group form one batch. Opaque and Cutout queues group equal keys
     anywhere in the front-to-back list (batches stay ordered by their nearest member, queues
     stay in order); Fade/Transparent keep their back-to-front order and only merge neighbours;
     shadow casters group freely (depth results don't depend on order). It is always on (no
     per-material "Enable GPU Instancing" toggle; `RenderOptions` can switch culling and
     instancing off for comparisons). Static batching (merging static meshes into one buffer)
     was not done: instancing already brings the stress scene to a few dozen draw calls and
     static batching would need per-scene combined meshes and memory.
278. **Identical images:** a headless test renders a small scene (four materials incl. cutout
     and fade, objects outside the view and behind the camera, sun + spot + point shadows,
     particles) with culling/instancing on and off and compares every pixel: they are equal.
     Opaque draw order inside a queue changes with instancing, which could only matter for
     exactly coplanar overlapping faces.
279. **Hot-path allocations removed:** the per-draw uniform list, the slot list and the batcher's
     map are kept between frames; terrain bounds are measured once per terrain version instead
     of scanning the heightmap every frame; shadow fitting reuses the culling boxes; the Game
     view Stats no longer look up every mesh's data each frame. Still per frame: the scene →
     draw list gathering (`GameView::new`, one `MaterialParams` clone per draw) and one terrain
     parameter buffer + bind group.
280. **Stats overlay** (Game view ▸ Stats, as before) now shows FPS and frame time, CPU main and
     render-call time, GPU time, Batches and Saved by batching, Tris/Verts, Screen, SetPass
     calls, visible shadow casters, visible/culled meshes, audio voices, physics/script
     counts. "Batches" counts every draw call of the render, shadow-map draws included.
281. **StressDemo is generated by code** (`sample_stress`, ignored writer + check + headless
     play test): 5,040 cubes and spheres on a 72 × 70 grid (four new "Stress *" materials, every
     25th object spinning with the existing `Rotator.lua`), a ground plane (Sand), a camera
     orbiting with the new `CameraOrbit.lua`, a soft-shadowed sun, a hard-shadowed spot and two
     point lights. No colliders (it measures rendering). The ignored
     `stress_benchmark` test renders it headless at 1280×720 with each optimisation on/off.

## 2026-11-03 — Input Manager, gamepads, Tags and Layers

282. **Input Manager axes live in ProjectSettings.json** (`"input": {"axes": [...]}`) with the legacy field set:
     name, descriptive names, negative/positive and alt buttons, gravity, dead, sensitivity, snap, invert, type
     (Key or Mouse Button / Mouse Movement / Joystick Axis), 0-based axis index and joystick number (0 = all).
     Files without the block get the default list (Horizontal, Vertical, Fire1–3, Jump, Mouse X/Y,
     Mouse ScrollWheel, then gamepad Horizontal/Vertical/Fire1–3/Jump, Submit ×2, Cancel). Entries may share a
     name; the one with the largest magnitude wins. Unknown axis / button names raise a Lua error (the script's
     callback stops), like the reference throwing.
283. **Axis smoothing runs per frame on real time** (`Time.unscaledDeltaTime`) in an `InputManager` stored in the
     scripting `World`, updated before `Start`/`Update`: key axes move towards the target at Sensitivity/s and
     back to 0 at Gravity/s, Snap restarts from 0 on reversal; mouse and joystick axes are raw × Sensitivity
     (joystick values inside Dead read 0, no rescaling). `GetButton*` only test the positive / alt positive
     buttons of Key or Mouse Button entries. Smoothing restarts with each scene load (new Runtime).
284. **Gamepads through `gilrs` 0.11** (pure Rust, Windows WGI/XInput, Linux evdev, macOS): no SDL to bundle and
     it reports pads with SDL-style mappings, so every pad looks like an Xbox controller. `input::Gamepads`
     polls once per game frame and keeps each pad in its slot while connected (an unplugged pad leaves an empty
     name, like `GetJoystickNames`). Fixed layout: buttons 0 A, 1 B, 2 X, 3 Y, 4/5 bumpers, 6 back, 7 start,
     8/9 stick clicks, 10 guide, 11–14 d-pad; axes 0/1 left stick (Y +1 = down), 2 triggers (left − right),
     3/4 right stick, 5/6 d-pad (Y +1 = up), 8/9 triggers 0..1. Joystick state is plain data on `Input`
     (`joysticks`), so tests inject fake pads and never open a device. The editor opens gilrs lazily on the UI
     thread (a leaked thread-local: dropping gilrs at process exit panics joining its worker thread) and
     passes pads to the game even without Game view focus (the editor doesn't use
     them); keyboard/mouse still need focus. No rumble, no per-device remapping UI.
285. **Button names**: keyboard names as before, plus `"mouse 0"`–`"mouse 2"`, `"joystick button N"` (any pad) and
     `"joystick J button N"` (1-based pad); they work in `GetKey*` and in axis button fields. `Input.anyKey` /
     `anyKeyDown` include mouse and joystick buttons. `Input.mouseScrollDelta` is in notches (50 px per notch,
     the player's wheel step; the editor's egui delta is approximate).
286. **Tags and Layers in ProjectSettings.json** (`"tags_and_layers": {"tags": [...], "layers": [32 names]}`).
     Built-in tags stay fixed and aren't stored; user tags are trimmed, unique and non-empty. Layers 0–7 are
     built in: 0–5 keep this engine's existing names and indices (Default, TransparentFX, Ignore Raycast, Water,
     UI, Player — old scenes keep their meaning), 6–7 are reserved and unnamed; 8–31 are user layers. Names are
     committed when a field loses focus. The Layer Collision Matrix, Inspector Layer dropdown ("N: Name" plus
     "Add Layer…") and Culling Mask list only named layers; Tag dropdown lists built-in + user tags plus
     "Add Tag…", both opening Project Settings ▸ Tags and Layers. Renaming or removing a tag/layer doesn't touch
     objects that use it. No Sorting Layers (the 2D work owns those) or rendering layers.
287. **Camera Culling Mask** (`culling_mask: u32`, default all bits, missing in old scenes = Everything) filters
     the game camera's meshes, terrains and particle systems by their object's layer; the Scene view and UI
     canvases ignore it. Inspector field is a Nothing / Everything / per-layer toggle dropdown; Lua
     `camera.cullingMask` reads/writes it. **Light culling masks are not implemented**: lights light every
     layer (the forward shader has no per-draw light mask; recorded as a gap).
288. **Lua tags**: `GameObject.FindWithTag` / `FindGameObjectsWithTag` return active objects in Hierarchy order and
     error on undefined tags; `LayerMask.*` use the project's names (built-in names when no game runs, e.g.
     while the Inspector reads script fields).
289. **InputDemo is generated by code** (`sample_input`, ignored writer + check + headless play with injected
     keys and a fake pad): a capsule character (`InputMover.lua`: GetAxis movement, GetButtonDown("Jump")), three
     steps to jump onto, a UI Text (`GamepadHint.lua`) naming the first gamepad, and a spinning "Hidden Marker"
     on user layer 8 "Hidden" that the Main Camera's Culling Mask leaves out. The writer names layer 8 in
     `SampleProject/ProjectSettings/ProjectSettings.json` (which therefore now lists every settings block).
     Reuses Checker, Wood, Blue Steel and Laser materials and `Rotator.lua` unchanged.

## 2026-11-03 — 2D: sprites, Sorting Layers, 2D physics

290. **Texture import settings are a `<image>.meta` JSON sidecar** next to the image (`Player.png.meta`), like the
     reference engine's per-asset meta files but holding only importer settings (no GUIDs). No sidecar = Texture
     Type Default, Bilinear, Repeat, mipmaps, which is exactly how textures behaved before, so existing textures
     and materials are unchanged. The Project window already hides `.meta` files. Moving an image outside the
     editor must move its `.meta` too.
291. **Settings:** Texture Type (Default / Sprite (2D and UI)), Sprite Mode (Single / Multiple), Pixels Per Unit,
     Pivot (presets + custom, Single mode), Filter Mode (Point / Bilinear; no Trilinear), Wrap Mode (Repeat /
     Clamp / Mirror), Generate Mip Maps, and the Multiple-mode sprite rects (name, pixel rect with y from the
     bottom, pivot). Switching to Sprite sets Clamp and no mipmaps, like the reference importer. The sampler of
     a material comes from its albedo texture's settings (Point filtering works for 3D materials too); the UI
     pass also honours them.
292. **Sprite references are strings:** the texture path, plus `#<name>` for a sprite of a Multiple texture
     (`Assets/Sprites/Tiles.png#Tiles_1`; no name = its first sprite). Built-in shapes are `Builtin/Square.png`,
     `Builtin/Circle.png`, `Builtin/Triangle.png`, generated in code (128 px at 128 PPU = 1 unit, anti-aliased
     edges) instead of shipping image files. Import settings and image sizes are cached per path and re-read
     when the image or `.meta` changes (checked at most twice a second).
293. **Sprite Renderers are drawn as mesh draws** (the unit Quad scaled to the sprite and offset by its pivot)
     with a fixed "Sprites-Default" material: unlit, Fade blending, Cull Off, no shadows, uv tiling/offset
     selecting the rect, flips done in uv space. They join the transparent queue, which now sorts by Sorting
     Layer, then Order in Layer, then back to front. 3D meshes stay at Default / 0, as in the reference engine.
     Only Draw Mode Simple; no custom sprite materials, masks, or sprite atlases. Unlit sprites still take scene
     fog (fog is off by default).
294. **Sorting Layers live in ProjectSettings.json** (`sorting_layers`, left out at the default `["Default"]`)
     with a Project Settings ▸ Sorting Layers page; the reference editor keeps them under Tags and Layers (the
     merger may move the page there). The renderer reads them process-wide (installed on load/change, like the
     project root); a layer's value is its index minus Default's, so Default is always 0. Renderers store the
     layer by name; unknown names sort as Default.
295. **2D physics is a separate rapier2d world** (`physics2d`), built at Play beside the 3D one and stepped in the
     same fixed-step loop right after it (same Fixed Timestep). Bodies use the object's world X/Y and the angle
     about Z; written back keeping the depth, rotation set to a pure Z rotation. Colliders attach to the nearest
     Rigidbody 2D up the hierarchy (else static), sizes × world X/Y scale, offsets baked into the collider pose.
     Mass comes from the Rigidbody 2D (density 0 colliders), forces are applied as velocity changes over one
     fixed step (as in 3D). Kinematic = velocity based; MovePosition sweeps a kinematic body there in the next
     step (velocity restored afterwards) and places other bodies; MoveRotation sets the angle directly.
     Kinematic/static contacts are not reported (like the reference default "Full Kinematic Contacts" off).
296. **Physics Material 2D is embedded in each collider** (friction 0.4, bounciness 0 defaults), like the 3D
     colliders' surface values; there is no `.physicsMaterial2D` asset. Polygon Collider 2D with no points uses
     the object's sprite rect (a unit square without one); concave outlines are decomposed into convex parts.
     Edge Collider 2D is a polyline (capsules per segment when Edge Radius > 0). Box edge radius rounds corners.
297. **2D layers reuse the 3D Layer Collision Matrix** and layer masks. Project Settings ▸ Physics 2D holds
     gravity, Queries Hit Triggers and Queries Start In Colliders (on by default as in the reference engine: a
     ray starting inside a collider reports it at distance 0, so ground checks should use a layer mask).
298. **2D callbacks:** `OnCollisionEnter2D/Stay2D/Exit2D(collision)` and `OnTriggerEnter2D/Stay2D/Exit2D(other)`.
     Stay comes from tracking touching pairs between started/stopped events (once per fixed step). The collision
     table has `gameObject`, `collider`, `otherCollider`, `transform`, `rigidbody`, `relativeVelocity` (other
     minus self, before the step for new contacts), `contacts` / `GetContact(i)` (one point: first manifold
     point, normal pointing towards the receiver), `contactCount`. Triggers pass the other GameObject.
299. **No Vector2 type in Lua:** 2D values are Vector3 with z = 0; `Vector2.new/zero/up/…`, `Vector2(x, y)` and
     every 2D function also accept `{x=, y=}` tables or two numbers. `Physics2D.gravity` is a live property
     (metatable). `ForceMode2D.Force/Impulse`. `Physics2D.OverlapCircle/OverlapBox/OverlapPoint` return the first
     GameObject (the `…All` variant of the circle returns a list).
300. **Editor:** GameObject ▸ 2D Object ▸ Sprite / Sprites ▸ Square, Circle, Triangle (placed at the pivot with
     z = 0). Sprites are picked by their rect (the top-most by sorting wins on one plane) and framed by it. The
     Rect tool on a selected sprite shows corner/edge handles that resize by scaling the object (opposite side
     fixed) and moves it when dragging inside. Box/Circle Collider 2D have an Edit Collider toggle with edge /
     rim handles; selected objects show green 2D collider outlines in the physics frame. No edit handles for
     Capsule/Polygon/Edge colliders (their points are edited in the Inspector).
301. **No separate Sprite Editor window:** Multiple-mode slicing is in the texture Inspector (Slice: Grid By Cell
     Size with pixel size, offset, padding, keep empty rects) with Revert / Apply and a preview outlining the
     sprite rects; unapplied edits live in egui memory. No automatic slicing, per-sprite rect editing, custom
     outlines or physics shapes.
302. **UI Image takes a sprite** (`sprite`, left out of files when unset): it replaces the texture and only its
     rect is drawn (no Image Type sliced/tiled/filled, no Preserve Aspect / Set Native Size).
303. **Platformer2D is generated by code** (`sample_2d`, ignored writer + check + headless play test): pixel-art
     PNGs made in code (Player, a 2-tile sheet sliced Multiple, Coin; Point filter, 16 PPU), tile platforms with
     one Box Collider 2D each, built-in Circle backdrops at order -10, an orthographic camera following the
     player (`CameraFollow2D.lua`), `PlayerController2D.lua` (A/D velocity, Space jump with three ground rays
     against Default), coins on Ignore Raycast as triggers that disappear and update a UI Text. It uses the
     Default sorting layer with orders, so ProjectSettings.json stays unchanged. The capture also accepts
     `MISTY_2D=1` to open the Scene view in 2D.

304. **Code Editor is a built-in layout tab.** At the user's request it now sits beside the Scene and Game tabs in every built-in layout (moved from `Tab::ON_DEMAND` to `Tab::ALL`). Saved layouts are not migrated: a user who closed it keeps their arrangement, and Window ▸ Layouts ▸ Default brings it back.

## 2026-11-04 — Soft shadows: PCSS and cascaded sun shadows

305. **Soft shadows are PCSS:** Shadow Type Soft = blocker search (Vogel disk, `textureLoad` of raw depth) →
     penumbra from the light size → variable-size PCF (bilinear compares on a Vogel disk) rotated per pixel by
     interleaved gradient noise of the screen position (deterministic, no temporal accumulation, so culling and
     instancing still render identically). Hard = one bilinear compare, None unchanged. The filter radius is
     clamped to 1.5–48 texels; blockers are searched up to 60 m above the receiver (sun). All of it is in
     `render/shaders/shadows.wgsl`, appended to `mesh.wgsl` (and so to the terrain shader).
306. **Light size per light:** directional `shadow_angle` (angular diameter in degrees, default 0.53°, the real
     sun), spot/point `shadow_radius` (metres, default 0.05), `shadow_bias` (texels, default 1) and
     `shadow_normal_bias` (texels, default 1.5). They are left out of scene files at their defaults, so existing
     scenes and their check tests are unchanged. The Inspector shows Shadow Angle / Radius only for Soft and the
     biases for Hard and Soft. `LightData` gained `shadow: LightShadow` (size, bias, normal bias; its Default has
     size 0, the narrowest soft filter).
307. **Receiver-plane depth bias:** wide kernels compare against the receiver's plane. Its depth slope per
     shadow-map uv comes from the surface normal and the shadow matrix (works for orthographic and perspective
     maps and needs no derivatives, so it is legal inside the light loop), limited to tan = 8 at grazing angles.
     The depth bias is `bias` texels of world distance converted to map depth.
308. **Cascades:** 1–4 (default 4) layers of a 2048² Depth32 array, practical split (λ = 0.8 between logarithmic
     and uniform) from the camera's near plane to min(Shadow Distance, camera far), default 150 m. Each cascade
     is an orthographic box around the slice's analytic bounding sphere (it depends only on fov and aspect, so its
     size never changes while the camera turns), with the radius quantised to 1/16 m plus two texels of padding.
     The light view has a fixed origin and the box centre snaps to whole texels, so shadows don't shimmer. The
     near plane is pulled back to the casters over the cascade's footprint (at most 1 km). Cascades overlap by
     Cascade Blend (10% of each) and the shader cross-fades across the band; shadows fade out over the last 10%
     of the shadow distance. This replaces the single map fitted around every caster.
309. **Per-cascade culling:** every cascade is a shadow map in the plan (`ShadowPlan::matrices()`: cascades, then
     local layers), so the profiler round's frustum culling and instanced batches apply per cascade unchanged.
310. **Quality settings live in Project Settings ▸ Quality** (`ProjectSettings.quality`, left out of the file at
     its defaults): Soft Shadow Quality Low/Medium/High/Ultra = 8/12/16/24 blocker and 12/20/32/64 filter samples
     (default Medium), Shadow Distance, Shadow Cascades, Cascade Split and Cascade Blend. `apply_globals` installs
     them process-wide (like Sorting Layers) and every renderer reads them, so the player uses them too.
311. **Soft-shadow parameters have their own uniform** (group 1 binding 6, `ShadowParams`) instead of new fields
     in the lights uniform; `Lights.shadow_view_proj` / `info.y` still hold cascade 0 for older code. Binding 2
     became a `texture_depth_2d_array` (one layer per cascade).
312. **Cascade debug view:** the Scene view draw mode "Shadow Cascades" tints the cascades red/green/blue/yellow
     (blended across the bands) through `Renderer::set_cascade_debug`, which applies to the next render only so
     the Game view never shows it. The capture also accepts `MISTY_DRAW_MODE=<draw mode name>`.
313. **ShadowsDemo is generated by code** (`sample_shadows`, ignored writer + check test): 6 m × 0.3 m poles, a
     figure built from primitives, a table under a spot light with a 0.5 m radius, pillars out to 180 m, and a
     20° sun with a 1° disc. It has its own materials (Shadow Ground / Pole / Figure). There is no play test
     (nothing is scripted); instead an engine GPU test renders it and checks all four cascade tints, and another
     measures the penumbra (≤ 8 px at a pole's base, at least 3× wider 11 m away; hard shadows don't widen).

## 2026-11-04 — Reflection probes and automatic placement

314. **Reflection Probe component** (GameObject ▸ Light ▸ Reflection Probe, Rendering category): Type Baked /
     Realtime, refresh interval (frames, 0 = on demand) and time slicing, importance, intensity, box projection,
     blend distance, box size and offset, resolution 64–512, near / far clip, clear flags Skybox / Solid Color,
     and the baked file path. The box is axis-aligned in world space (rotation and scale ignored), like the
     reference engine. Every field is `serde(default)`, so old scenes load unchanged.
315. **Baking uses the engine's own renderer:** six 90° views from the probe into an HDR target, read back
     before bloom and tonemapping (the target's HDR texture gained `COPY_SRC`). The camera rotation per face is
     chosen so the rendered image *is* the cube face in wgpu's convention (no flipping). The first pass sees no
     probes; the optional second pass (generator setting, on by default) re-captures with the first results,
     so probes see each other's reflections. Every active renderer is captured, not only Static ones.
316. **Prefiltering and SH on the CPU** (`render/probes/prefilter.rs`, `sh.rs`): seven roughness levels
     (perceptual roughness i/6), GGX importance sampling with N = V = R and filtered importance sampling from a
     box-filtered source chain (64 samples per texel for bakes, 24 for realtime), split over threads by face.
     SH9 is projected from a 32² copy and convolved with the cosine lobe so it evaluates to irradiance / π.
     CPU code is easy to test and leaves the GPU passes (which three other agents are editing) alone; a bake of
     the demo's 10 probes × 2 bounces takes ~13 s in a dev build.
317. **Probe file `ReflectionProbe-N.probe`** next to the scene (`Assets/Scenes/<Scene>/`, like the nav mesh):
     `MPRB` header, SH as f32, then zlib of every level as RGB half floats with the bytes split into low/high
     planes (about 3× smaller than raw halves; ~280 KB per 128² probe). No EXR dependency. Files are loaded
     through a cache that re-checks the modification time twice a second, in the editor and the player; stale
     higher-numbered files are deleted after a bake.
318. **GPU layout:** one cube-array atlas (Rgba16Float, 7 mips, face size = the largest probe, others resampled)
     holding every probe of the view, a uniform with up to 64 probes (position, box, blend, flags, SH), a
     storage buffer with each draw's four best probes, and a 32² split-sum BRDF LUT computed on the CPU. They
     are bindings 20–24 of the lit-mesh group (`probes/gpu.rs`), far from the existing numbers so other
     renderer changes merge cleanly. `Frame` is unchanged: probes travel in `Environment::probes`.
319. **The terrain pass binds an empty layout at group 2.** It never used the material textures, and with the
     probe atlas and LUT the terrain pipeline would exceed the default 16 sampled textures per stage.
320. **Per-draw lists, per-pixel blending:** the CPU gives each draw the up-to-four probes whose boxes touch its
     bounds, most important first, then smaller box first. The shader walks that list, weighting each probe by
     its blend distance inside the box and giving the remainder to the next; whatever is left falls back to
     the sky. No clustered grid: scenes have tens of probes, not thousands.
321. **Shading:** specular samples the cube level of the surface's perceptual roughness along the box-projected
     reflection (ray against the box, seen from the capture point) and multiplies by the split-sum
     `F0·A + B` with a multiple-scattering energy term; diffuse ambient is the probes' SH instead of the
     hemisphere sky colour, so rooms get darker, wall-tinted ambient. Where no probe covers a pixel the old
     sky code runs unchanged, so scenes without probes render exactly as before.
322. **Automatic placement** (`placement.rs`): candidate columns on a grid at the spacing over the bounds of every
     active renderer and terrain; a BVH ray down each column finds every up-facing floor; probes sit at the
     height setting (1.5 m) above it, lower under low ceilings; candidates inside solids (four of six axis rays
     hit back faces) or within 0.2 m of a surface are dropped. Each box side is the median of a 3 × 3 fan of
     rays (offset 0.25 × spacing, at most 1 m), so a doorway or a chair doesn't decide alone; box faces sit
     0.1 m beyond the walls so the walls are fully inside. A side that hits nothing within 4 × spacing is open
     and reaches 0.75 × spacing (4 m upwards).
323. **Merging and roles:** fully enclosed probes that sit inside each other's boxes are one room and merge into
     one probe at their mean (refitted); open probes merge with neighbours up to two cells away at the same
     height and cover their members' boxes. Enclosed probes get box projection, importance 2 and a 0.1 m blend;
     open ones get no box projection, importance 1 and a quarter-spacing blend. Roofs count as floors (a probe
     above a flat roof is kept).
324. **Generate Probes is undoable and re-generatable:** it replaces the root object carrying a
     `Reflection Probe Group` component ("Reflection Probes", which also stores spacing, height, merge,
     resolution, second bounce) and then bakes when the scene is saved. Bakes run one probe per frame through
     `probe_tools::gpu_frame` after the panels (like material previews), with a progress bar in the Lighting
     window; Realtime probes are captured there too (one face per frame with time slicing, prefiltered on a
     worker thread).
325. **Gizmos:** each probe shows a small chrome sphere drawn with the normal lit shader (so it reflects exactly
     what the probes give that point), selected probes show the box and the inner blend box, and the
     Inspector's Edit Bounding Volume toggles six face handles. The Scene view draw mode "Reflection Probes"
     colours pixels by the probe(s) lighting them (dark grey = sky) and shows every box.
326. **ReflectionsDemo is generated by code** (`sample_reflections`): a room with red/green/blue walls, a doorway,
     chrome / gold / copper spheres on glossy tiles, a lamp, an outdoor lawn with a big chrome ball; probes from
     the placement code with the files baked by the ignored writer on the GPU. The check test re-runs the
     placement and compares the scene; a headless render checks the probes change and darken the room. The
     capture also takes `MISTY_VIEW=x,y,z,pitch,yaw` and `MISTY_DRAW_MODE=Reflection Probes`.

## 2026-11-04 — Volumetric fog and light shafts

327. **Froxel fog in compute, applied in the forward shaders.** A camera-aligned grid (Low 96×54×48, Medium
     128×72×64, High 160×90×64) with exponential view-depth slices (`near · (far/near)^w`, near = reach/128,
     slice 0 starting at the eye) is filled by one compute pass (density + lighting + temporal blend, 4×4×4
     threads per group) and integrated front to back by a second (one thread per column, energy-conserving
     `S·(1−e^(−σΔ))/σ` per slab). There is no depth prepass, so every froxel is computed (no empty-froxel
     culling). Meshes, terrain, sky/background and particles sample the integrated volume at their own world
     position through `fog_apply.wgsl` (trilinear; texel k holds the sum to the far side of slice k, the first
     half slice fades in linearly), as the fog has to reach transparent and particle draws in a forward
     renderer anyway.
328. **Fog resources live in the view bind group (group 0, bindings 1–3)** next to the view uniform, because
     every forward pipeline already binds it and the terrain pipeline has no free group (4 max). The fog
     uniform is its own buffer, so the `View` struct and `VIEW_STRIDE` are unchanged. To stay under the 16
     sampled textures per stage of the default limits, the terrain pipeline's unused group 2 (it bound the
     5-texture material layout) is now an empty group.
329. **Shared integrated volume, per-view history.** The integrated volume is one texture in the renderer (each
     render fills it right before its own main pass, renders are submitted in order). The lit-froxel history is a
     ping-pong pair per render target, keyed by a new `RenderTarget::id`; state of targets not rendered for 600
     renders is dropped, and a view's history is dropped as soon as it renders without volumetric fog.
330. **Temporal reprojection:** each froxel is sampled at a Halton(2,3,5) position inside its cell (16-frame
     cycle) and blended 0.9 with last frame's value fetched (trilinear) where its centre was in last frame's grid;
     history is ignored on the first frame, after a quality or reach change and outside last frame's grid.
     Denoising off = centre samples, no history. Noise animates with real time, so moving noise trails a little
     with denoising on (the same trade-off as the reference pipeline).
331. **Density:** global fog with extinction 1/`Fog Attenuation Distance`, uniform below `Base Height` and
     `e^(−2(h−base)/(max−base))` above (≈ 1/8 left at `Maximum Height`); Fog Volumes add `1/Fog Distance` inside
     their oriented box with a linear soft edge of `Blend Distance` metres per face (product of the three axes),
     up to 16 per view; optional 3-octave value noise (`smoothstep(0.3, 0.7, n)·2`, mean ≈ 1, so intensity
     breaks the fog up without thinning it overall) scrolled by the wind vector. Noise and boxes only exist in
     the volumetric path, like local fog in the reference pipeline.
332. **Lighting units:** lights are in surface units (a white wall facing a light shows its colour), so the
     in-scattered radiance is `σs · Σ light · phase_HG(g, cos) · π` (an isotropic medium returns a quarter of a
     white wall), plus ambient = the scene's Environment Lighting averaged over the sphere (sky + 2 × equator +
     ground)/4 × intensity × `Ambient Light Probe Dimmer` (there are no light probes yet; the name keeps the
     familiar control). Spot/point falloff matches the surface shader. Up to 8 lights per view.
333. **Shadows through one accessor** (`volumetrics/shadow_access.rs`): the directional map, the local layer
     array and their matrices from the `ShadowPlan`, so the cascade / soft-shadow work only has to adapt that
     file. Froxels use one hardware-PCF compare per light (a small constant bias, no normal offset: froxels have
     no surface). `Volumetric Shadow Dimmer` lerps the shadow towards lit; `Volumetric Multiplier` scales the
     light in fog only (0 skips the light).
334. **Analytic fallback and distance handling:** with Volumetric off (or in orthographic views, where froxels
     don't fit), shaders integrate the same height-fog density analytically along the view ray (split at the
     base height) with unshadowed sun + ambient light. With Volumetric on, froxels cover the first `Volumetric
     Distance` (default 64 m, an extra field beyond the listed ones) and the analytic fog continues to `Max Fog
     Distance`; anything farther, the sky and camera background included, is fogged as if it were at Max Fog
     Distance, so distant geometry never looks foggier than the sky behind it.
335. **Fog defaults to off everywhere:** the Fog and Sun Shafts overrides are inactive in new and old volumes and
     are left out of saved files while untouched; the new Light fields (`volumetric_multiplier`,
     `volumetric_shadow_dimmer`, default 1) are left out while 1, so every existing scene and its check test
     stay byte-identical. With fog off the shaders return the input colour unchanged; a headless test renders
     identical pixels to a renderer that never had fog (and the old culling test image is byte-identical to the
     one before this work).
336. **Settings flow without changing `Frame`:** the Fog / Sun Shafts values ride on `PostSettings` (`fog:
     FogSettings`, `sun_shafts`), and Fog Volumes on `Environment::fog_volumes` (gathered by
     `render_scene::environment`). `LightData` gained `volumetric: VolumetricLight` (Default = 1, 1). The
     highest-priority volume still wins as a whole (overrides aren't blended between volumes; there are only
     global volumes).
337. **Sun Shafts are a separate screen-space effect:** half-resolution prefilter of pixels whose depth is still
     cleared (sky / background) and brighter than `Threshold`, weighted by distance to the sun (`Radius`), three
     8-tap radial blurs towards the sun (full, 1/8 and 1/64 of `Length`, ≈ 500 effective taps), added onto the
     HDR image before bloom, tinted by the sun colour × `Tint` × `Intensity`. They fade out as the sun leaves
     the screen and are skipped when it is behind the camera. The main pass keeps its depth (Store instead of
     Discard) only on frames that draw shafts; the depth texture became sampleable for that.
338. **Scene view:** the Fog effects toggle now also controls the volumetric fog (it no longer needs Post
     Processing on); the draw-mode menu gained **Fog Density** (1 − transmittance) and **Fog Lighting** (in-
     scattered light only), which always show the active volume's fog. A selected Fog Volume draws its box and
     the inner box where the soft edge ends; Fog Volumes get a ☁ scene icon so they can be clicked.
339. **VolumetricFogDemo is generated by code** (`sample_volumetric_fog`, ignored writer + check + headless
     play): a ruined wall with window slits and a forest of trunks against a low sun (god rays), a street lamp
     spot light (Volumetric Multiplier 1.5), a lantern point light inside an emissive globe that doesn't cast
     shadows, a dense blue Fog Volume between two ridges, ACES + bloom, Fog at High quality with light noise,
     and the shared `CameraOrbit.lua` circling the forest.
340. **Fog + soft shadows + probes merge:** the froxel lighting samples the sun's 4-cascade array (froxel
     binding 4 is a `D2Array`; the fog uniform carries every cascade matrix and the cascade count). Each
     froxel takes the first (tightest) cascade whose map contains it; cascades are nested around the camera,
     so this matches picking by view depth without passing depth splits. All fog access to the shadow system
     stays in `volumetrics/shadow_access.rs`. Scene view draw modes are one indexed list (Shaded, Wireframe,
     Shaded Wireframe, Shadow Cascades, Reflection Probes, Fog Density, Fog Lighting) with a named constant
     per index; `MISTY_DRAW_MODE` matches by name. Sampled textures per fragment stage stay under the default
     16: mesh 10 (fog volume, cascades, local shadows, probe atlas, BRDF LUT, five material maps), terrain 14
     (group 2 empty), so the device limits are unchanged.

341. **Screen-space AO is horizon based (GTAO-style), written from the published math:** per pixel, 2 slice
     directions per frame with temporal accumulation (4 fixed ones without), `Quality` horizon steps per side
     (squared spacing, denser near the pixel), occluders fading over the outer 60% of the radius, the
     cosine-weighted arc between the two horizons, then `visibility ^ Intensity`. Runs as three compute passes
     (`render/screen_space.rs`, `shaders/ao.wgsl`): trace at half or full resolution, a 5×5 (7×7 without
     history) depth- and normal-aware blur, and a full-resolution resolve with a depth-weighted bilateral
     upsample. Pixels no half-resolution texel saw (thin edges) get no AO rather than the AO behind them.
342. **Depth + normal prepass only while an effect is on:** the renderer is forward + 4× MSAA with no normal
     buffer, so opaque/cutout meshes (the camera's own batches) and terrains are drawn once more into a
     single-sample Rgba16Float world-normal target and Depth32 buffer. Vertex normals only (normal maps are
     not in the prepass). With both effects off nothing extra runs and the image is bit-identical (GPU test).
343. **AO and depth reach the lit shaders as one storage buffer** (`@group(1) @binding(16)`, a 64-byte header
     plus (AO, linear depth) per pixel) instead of textures: the terrain pipeline already uses all 16
     sampled textures of the default limits, and a buffer needs no per-pass layout change. Two ping-pong
     buffers per view hold the current frame and the history. Group 1 is rebuilt per render when an effect is
     on (the buffers are per target); otherwise the renderer's zero-header buffer is bound.
344. **Temporal accumulation:** last frame's AO is reprojected with the previous view-projection and blended
     at 0.9 when its stored depth matches the expected one (perspective: last frame's view depth; 3% + 2 cm).
     A camera cut (moved over 5 m or turned over ~32° in a frame), a resize, a resolution change or Temporal
     off drops the history; that frame uses the wider spatial blur instead (no TAA in the engine).
345. **AO applies to ambient light only, like HDRP:** ambient diffuse gets `min(material occlusion, SSAO)`
     with a multi-bounce fit on the diffuse albedo (bright surfaces lose less); sky reflections get a
     specular occlusion derived from it (Lagarde's approximation); direct light is multiplied by
     `mix(1, SSAO, Direct Lighting Strength)` (default 0). With SSAO off the material occlusion path is the
     old expression exactly. Transparent surfaces (not in the prepass) are left unoccluded: the lookup only
     applies when the fragment's depth matches the prepass texel.
346. **Contact shadows march in the lit shader**, per light with its Light Inspector toggle (`LightItem.spot.z`):
     `Sample Count` jittered steps along `Length` towards the light, a hit when the sample is behind the
     prepass surface by more than a small bias and less than a thin thickness (two steps, at least 10 cm,
     + 1% of the depth). They fade over the last 20% of `Distance Fade` and with grazing light (so round
     shapes don't shadow their own terminator); `Opacity` scales them. They apply where the shadow map
     applies (Receive Shadows).
347. **Overrides stay out of scene files until changed:** `ambient_occlusion` / `contact_shadows` on the
     Post-process Volume and `contact_shadows` on Light use `skip_serializing_if` default, so every existing
     scene keeps its exact text and loads with both effects off. Like the other overrides, the
     highest-priority enabled volume decides (no per-field blending across volumes yet).
348. **Ambient Occlusion draw mode:** the Scene view's draw-mode menu gets "Ambient Occlusion": lit surfaces
     output the SSAO as grey with no grading; it uses the scene's AO settings switched on (defaults when no
     volume has them). The capture also accepts `MISTY_DRAW_MODE=Ambient Occlusion` and `MISTY_ALIGN_VIEW=1` (Scene
     view camera at the selected object, e.g. the Main Camera).
349. **AmbientOcclusionDemo is generated by code** (`sample_ao`, ignored writer + check + headless play +
     headless GPU render test): corner room, stacked crates, columns on plinths, resting spheres, cubes at
     gaps 0 / 0.1 / 0.3 / 0.6 / 1.2 m, a sphere bobbing 2 m up and down (`VerticalBobber.lua`), sun over the side wall
     with contact shadows, a volume with AO (intensity 2, radius 2 m, 8 steps, half resolution, temporal),
     Contact Shadows (0.3 m) and Neutral tonemapping. Its own `AO *.mat` materials.
350. **AO + fog + soft shadows + probes merge:** `LightData` carries `shadow`, `volumetric` and
     `contact_shadows`; the mesh group holds the soft-shadow params (6), the AO/contact texel buffer (16)
     and the probes (20–24), and `draw_groups` takes both the probes and the screen-space buffer (the per-frame
     lit group the AO pass builds includes the probes too). Specular sky and probe reflections both use the
     AO specular occlusion, probe diffuse the indirect occlusion; contact shadows multiply the soft shadow.
     "Ambient Occlusion" is draw mode 7 in the one Scene view list, and `MISTY_DRAW_MODE` still matches by
     name (the AO branch's numeric form was dropped). AO adds no sampled textures to the forward passes
     (storage buffer), so the 16-texture default limit still holds.

## 2026-11-04 — Player, Quality and the build pipeline

351. **Player settings live in ProjectSettings.json (`player`, absent until first edited).** Projects made before
     the Player page keep working: `ProjectSettings::player_or` builds Player settings from the old Build
     Settings fields (product name, size, fullscreen → Fullscreen Window / Windowed). The old fields stay in
     BuildSettings.json for loading and are no longer shown; a build packs the migrated settings. New projects
     default to Fullscreen Window, VSync on, Target Frame Rate -1, Resizable, Run In Background off, version
     "1.0", company "DefaultCompany" (as in the reference engine).
352. **Screen modes:** Fullscreen Window = winit borderless fullscreen; Exclusive Fullscreen = the monitor's video
     mode closest to the default resolution (highest refresh among equals), borderless if none; Maximized =
     a maximised window. Run In Background off pauses updates and rendering while unfocused (the paused time
     isn't one long frame). Target Frame Rate applies only with VSync off (`ControlFlow::WaitUntil`, catching
     up at most one frame). The splash screen is the image fitted in 80% of the window over its background
     colour for the duration, skipped by automated `MISTY_SCREENSHOT` captures; no engine logo is forced.
353. **Quality levels are stored in ProjectSettings.json (`quality`, left out at defaults)**: Low / Medium / High
     / Ultra, High is the default. Fields: shadow resolution (512–4096), distance, cascades (1/2/4), soft
     shadows, MSAA (off/2x/4x), texture quality (= the global mipmap limit: Full/Half/Quarter/Eighth),
     anisotropic textures (Disabled / Per Texture / Forced On), max texture size, render scale, LOD bias, and
     AO / volumetric fog / reflection probe tiers. VSync is a Player setting only (one place to look).
354. **The active level is process-wide** (`misty_engine::quality::install/set_level/active/generation`, like the
     Sorting Layers global), so Lua, the player and the editor preview share it. The renderer gets a copy via
     `Renderer::set_quality` (a RenderOptions-style hook) and readers use `Renderer::quality()` and the
     `QualityLevel` accessors (`shadow_map_size`, `local_shadow_map_size`, `soft_shadow_samples`,
     `msaa_samples`, `ao_samples`, `fog_slices`, `reflection_probe_size`, `texture_size_limit`, `anisotropy`).
     Wired now: the directional shadow map size (maps and bind groups rebuilt on change, the normal-offset
     texel rescaled), render scale (player target and Game view), texture size limit and anisotropy. Stored
     for the passes being written in parallel: shadow distance and cascades, soft shadows, AO, fog, probes,
     LOD bias. MSAA is stored only: the pipelines use a fixed 4x sample count.
355. **Render Scale scales the whole game image, UI included** (the UI is drawn into the same target); UI
     clicks are mapped into target pixels in both the player and the Game view. Canvas Scaler "Scale With
     Screen Size" keeps the same layout; Constant Pixel Size UI looks larger at scales below 1.
356. **Quality preview in the editor is a Game view toolbar dropdown**: it switches the active level for the
     Scene and Game views and Play mode. Opening a project activates the default level.
357. **Texture import extended, not duplicated:** Filter Mode is Nearest Neighbor (no filter) / Bilinear /
     Trilinear; `"Point"` in old `.meta` files loads as Nearest Neighbor. Bilinear really is bilinear now
     (nearest mip) and Trilinear blends mips; textures without a `.meta` are Trilinear (what "Bilinear" did
     before), so they look the same. New fields with serde defaults: Max Size (32–8192, default 2048, Lanczos3
     downscale on load), Aniso Level (0–16, default 1), sRGB (default on) and Texture Type Normal Map (always
     linear). Choosing Sprite turns mips off and clamps; Default / Normal Map turn mips on. The CPU mip chain
     (linear-light box filter) already existed and is used by the Scene view, Game view and player alike.
     No Read/Write flag (scripts can't read texture pixels, so it would do nothing).
358. **Anisotropy needs trilinear sampling** (wgpu rejects anisotropy with any nearest filter), so Nearest and
     Bilinear textures never get it. Quality ▸ Anisotropic Textures: Disabled = 1x, Per Texture = the texture's
     Aniso Level, Forced On = at least 8x. High and Ultra default to Forced On, which keeps the 8x every
     texture had before. Texture Quality and Max Texture Size cap the uploaded size on top of the texture's own
     Max Size; changing the level reloads every texture (cache keyed by `quality::generation`). Apply in the
     texture Inspector reloads the texture live (the `.meta` change is picked up by the texture cache).
359. **Asset file system (`vfs`):** every asset loader (scenes, prefabs, materials, textures and `.meta`, models
     incl. `.mtl`/`.bin`, audio, scripts, animations, controllers, mixers, terrains, nav meshes, settings) reads
     through `misty_engine::vfs`. Unmounted it is `std::fs`; with a pak mounted, paths under its root are served
     from the archive (directory listings merge pak and disk). Loose project folders keep working for
     development (`misty_player <Project>`).
360. **The pak format is custom and tiny** (`MISTYPK1` magic, index at the end: path, offset, stored size, size,
     codec). Entries are LZ4 (lz4_flex, pure Rust, so every target builds) when that saves 1/16, never for
     PNG/JPG/OGG/MP3/GLB. zstd was not used: its C build complicates cross-compiling. Entries are read on
     demand, not loaded into memory at once.
361. **Only referenced assets ship.** `asset_deps` walks from the enabled build scenes (plus the icon and splash
     image): every "Assets/…" string (sub-asset `#…` removed) in JSON assets, string literals in Lua, `mtllib`
     and texture maps of OBJ/MTL, buffer/image URIs of glTF, each file's `.meta`, and everything under any
     `Resources` folder. Missing references are build-log warnings, not errors.
362. **Build output:** `<Product>.exe` (the player renamed; no suffix on Linux; on macOS `<Product>.app` with an
     Info.plist and the data in `Contents/Resources`), `<Product>_Data/data.pak` (assets + ProjectSettings.json
     + BuildSettings.json + BuildInfo.json), `<Product>_Data/icon.png` (+ `icon.ico` on Windows). The player
     finds `<exe stem>_Data/data.pak`, falling back to any `*_Data` folder with a pak (a renamed exe still
     works). The old loose `Assets/` + `ProjectSettings/` build layout still plays.
363. **The icon is embedded with the Win32 resource API** (`BeginUpdateResourceW/UpdateResourceW`, declared
     inline, no crate): RT_ICON images of 16–64 px as 32-bit DIBs and 256 px as PNG, plus an RT_GROUP_ICON. It
     works for x64 and ARM64 executables but needs a Windows host; elsewhere the build logs a warning and the
     window icon (set at runtime from the packed PNG) still works. No `.icns` for macOS bundles yet.
364. **Building compiles the player with cargo** in the engine workspace (`CARGO_MANIFEST_DIR` at editor compile
     time, or `MISTY_ENGINE_SOURCE`): `cargo build -p misty_player --release --target <triple>` (or
     `cargo zigbuild` for Linux when installed), on a background thread with stderr streamed into the window's
     log and progress from "Compiling" lines against the Cargo.lock package count. Without the source, the
     player next to the editor is used, for the host platform only. Always `--target`, so outputs land in
     `target/<triple>/release` and never clash with the running editor.
365. **Target checks:** `rustup target list --installed` (once, in the background; "Check again" re-runs it)
     decides ✔ / ⚠ and shows `rustup target add <triple>` with a Copy button. macOS from another OS, and Linux
     from another OS without cargo-zigbuild, are reported as blocked with the reason before cargo runs.
     Same-OS other-CPU builds show a note about the C tools (MSVC ARM64 build tools, a cross gcc).
366. **Development Build** = a console window (AllocConsole) and `Player.log` next to the exe, plus
     `steam_appid.txt` when a Steam App ID is set. Release builds have no console and log to the per-user
     folder (the install folder may be read-only). Both are optimised (`--release`).
367. **PlayerPrefs are a JSON file per user and game** (`%APPDATA%/<Company>/<Product>/prefs.json`, macOS
     Application Support, Linux XDG config), written on `PlayerPrefs.Save()` and when the player quits. The
     player keeps the chosen quality level and resolution there (`Misty.*` keys) and reapplies them at
     startup. In the editor, Play mode uses the project's `Library/PlayerPrefs.json`.
368. **Lua:** `Screen.width/height/fullScreen/fullScreenMode/resolutions/SetResolution(w, h, mode)` with
     `FullScreenMode.ExclusiveFullScreen/FullScreenWindow/MaximizedWindow/Windowed` (0–3) or a boolean;
     `QualitySettings.GetQualityLevel()/SetQualityLevel(i)/names` (indices from 0); `PlayerPrefs.GetInt/
     SetInt/GetFloat/SetFloat/GetString/SetString/HasKey/DeleteKey/DeleteAll/Save`. In the Game view
     `SetResolution` does nothing (there is no window to resize).
369. **Steam is upload scripts only:** with a Steam App ID the build writes `steam/app_build_<app>.vdf`,
     `steam/depot_build_<app+1>.vdf` (content root = the build folder; excludes `steam/*`, `steam_appid.txt`,
     `Player.log`) and `steam/README_STEAM.md` (steamcmd and Steamworks steps). Real Steamworks API
     integration (achievements, overlay, workshop) and downloading the SDK are out of scope for now.
370. **Zip for itch.io and the like:** a small deflate writer (flate2 + crc32fast, already in the lock file)
     instead of the `zip` crate; entries carry Unix permissions so Linux/macOS executables stay executable.
     Named `<Product>-<version>-<platform>.zip`, next to the build folder, without the Steam files.
371. **Project Settings pages were merged to keep the list short** (the user asked for fewer tabs): Player,
     Quality, Input, Physics (a "3D Physics" section plus a collapsed "2D Physics" section), Tags and Layers
     (with a collapsed "Sorting Layers (2D draw order)" section), Time. Each page has a one-line description
     and collapsible sections with tooltips; nothing was removed. Old page names still work for
     `MISTY_SETTINGS` and links ("Input Manager", "Physics 2D", "Sorting Layers"). The texture Inspector keeps
     Type, the sprite fields, Max Size, Filter Mode and mipmaps on top and puts Wrap Mode, sRGB and Aniso Level
     under a collapsed "Advanced".
372. **The Build window keeps Player options out:** it shows the scenes, a platform list with status, four
     options (Development Build, Compress Data, Create .zip, Steam App ID), a one-line summary of the Player
     settings with a "Player Settings…" button, a progress bar and a collapsible log.
373. **One quality struct for everyone:** `QualityLevel` in `misty_engine/src/quality.rs` also carries Cascade
     Split (0 = even … 1 = logarithmic) and Cascade Blend so the soft-shadow work's per-level fields (soft
     shadow quality, distance, cascade count, split, blend) live on the same Quality page; the editor shows
     split/blend only with more than one cascade. Built in parallel with the soft-shadow branch, which has
     its own `quality.rs`: at merge, fold its fields into `QualityLevel` and read them through
     `Renderer::quality()` / `quality::active()`.
374. **One quality system (Player/Quality/Build merge):** the soft-shadows work's `QualitySettings { shadows }`
     (installed by `apply_globals`, read through `quality::shadows()`) is replaced by the Build branch's
     `QualityLevel` list (Low / Medium / High / Ultra, one default, `quality::install` / `active`, and
     `Renderer::set_quality`). There is one Quality page. The shadow pass reads
     `QualityLevel::shadow_settings()`: cascades (1, 2, 4), Shadow Distance, Cascade Split, Cascade Blend,
     Shadow Resolution (the 4-layer cascade array is reallocated at that size) and Soft Shadows (Off draws
     Soft lights hard; Low / Medium / High = 8/12, 12/20, 16/32 blocker/filter taps, the old shadow quality
     presets). The AO, Volumetric Fog and Reflection Probes tiers were already fields of `QualityLevel`;
     AO Off turns screen-space AO off. The fog and probe tiers are stored for now; per-volume Fog Quality and
     the probes' own resolution still decide those passes.

375. **Auto Paint rules live on each layer** (`TerrainLayer.rules`, serde defaults so old `.terrain` files
     load): slope band (degrees), optional height band (metres above the terrain base), Spread 0..1 (falloff
     past the band edges: 0.5° + 30° × spread for slope, 1 % + 15 % of the terrain height for height), Breakup
     strength + scale (seeded 4-octave value-noise fBm that shifts the band edges by up to strength × (falloff
     + 10°) / (falloff + 6 % height)), Priority. `rules.enabled` *is* the rule-driven flag: disabled layers
     are manual and Generate keeps their weight per texel; the rule-driven layers share what is left, stacked
     by priority (higher covers lower), with the lowest rule-driven layer filling the remainder. One seed per
     terrain (`rule_seed`); each layer offsets it so edges differ.
376. **Generate is CPU, multithreaded with `std::thread::scope`** (rows split over all cores, no new
     dependency) and one undo step (a full snapshot like the other terrain edits). "Generate in brush" is a
     Paint Texture mode, not a separate tool: the brush moves texels towards the rule result at the paint
     rate, for layers and grass, and is undoable as a stroke.
377. **Grass uses the same rules** (`DetailSettings.rules`) plus an optional layer to grow on, a density map
     per splat texel filled by Generate (`detail_map`, base64 in the file) and a hard slope limit (band max +
     spread: breakup can thin grass but never push it onto cliffs). Blades are scattered deterministically per
     16 m chunk around the camera only (within the draw distance, 24 chunks per frame), so huge terrains never
     hold millions of blades. There are no tree instances in Misty terrains yet, so Generate scatters grass only.
378. **Grass renderer:** one instanced 7-vertex strip per blade (3 tapered segments + tip), lit by the mesh
     shader's `shade` (shadows, fog), wind as two sine waves travelling along a fixed breeze direction, and a
     distance fade that shrinks blades to nothing between 60 % and 100 % of the draw distance (no alpha
     sorting). Blades do not cast shadows. Colours, wind and draw distance are uniforms (no re-scatter).
379. **Terrain rendering moved to vertex pulling:** the heightmap is an R32Float texture and normals an RGBA8
     snorm texture; there is no terrain vertex buffer. Chunks of up to 64 cells draw shared index patterns with
     the chunk's first sample as base vertex (WGSL `vertex_index` includes it). LOD halves the sample density
     per level by distance (1.5 chunk sizes per level); neighbours differ by at most one level and the finer
     side snaps its odd edge vertices onto the coarse grid (16 edge variants per LOD, no skirts, no cracks).
     Shading uses the per-pixel normal texture, so LOD changes the silhouette only. Shadow casters use a
     dedicated `terrain_shadow` pipeline with the same vertex code (`terrain_vertex.wgsl`).
380. **Partial uploads via a store change log:** `store::modify_region` records the normalised rectangle a
     brush touched; the renderer asks `changes_since(its version)` and re-uploads only those heights, normals
     and splat texels (`queue.write_texture` of the rectangle), updates the bounds incrementally and drops
     the grass chunks there. `modify_settings` (empty region) is used for layer/cliff/harmony/detail settings
     so dragging a slider re-uploads nothing; plain `modify`, `insert` (undo) and file loads are full uploads.
     The log keeps 256 entries; readers older than that do a full upload.
381. **Heightmap resolutions 2049 and 4097, splat 1024 and 2048** were added for large terrains. The `.terrain`
     format stays JSON + base64 (a 4097² file is ~45 MB); saving it on mouse-up is the remaining hitch.
382. **Layer textures are 2D arrays** (albedo sRGB, normal, height; one slice per layer, resized to one
     power-of-two side ≤ 1024 with CPU mips), loaded by the terrain renderer itself and polled for file
     changes twice a second. This keeps the terrain pipeline at 5 fragment textures instead of 9 (the default
     limit of 16 per stage was already reached with the material and shadow groups). Texture import settings
     are not applied to terrain layers.
383. **Cliffs:** `CliffSettings { triplanar (on), triplanar_angle 45°, sharpness 4, height_blend 0.5 }`.
     Projection weights are |n|^sharpness, blended in from the top projection over ±5° around the angle and
     dropped below 2 % (each skipped projection saves 3 texture reads); gentle slopes sample only the top
     projection, and with triplanar off the shader never takes the side branches (the cheap path). Texture
     gradients come from world-position derivatives so per-pixel branches can use `textureSampleGrad`. Side
     normals use a whiteout blend per axis. Height Blend is the classic height-lerp: weights become
     max(h + w − (max(h + w) − window), 0) with window = 1 − 0.97 × contrast; contrast 0 is the plain blend.
     Layers without a Height Map use their albedo luminance stretched to 0..1.
384. **Color Harmony is analysis on the CPU, adjustment in the shader:** each layer's mean albedo (linear,
     from the loaded image) and its splat coverage give a shared target in HSV: hue = circular mean weighted by
     coverage × saturation (greys carry no hue), saturation and value = weighted means. Tint and Lighting
     targets replace only the hue (sun colour for Lighting). Per layer the shader gets a hue shift and
     saturation / brightness scales = Strength × the Hue/Saturation/Brightness weights of the way to the
     target, applied per sample in HSV; Strength 0 (or Preview Before, which is not saved) passes the exact
     identity and the shader skips the conversion. Source textures are never modified.
385. **UI stays in the existing tabs:** everything procedural is in Paint Texture as collapsible sections
     (the selected layer's Auto Paint Rules, Generate row with Seed and "Generate in brush", Grass & Details,
     Cliffs & Blending, Color Harmony); no new tool tabs. Slope / height / blade-height ranges use a small
     two-handle slider with numeric fields.
386. **TerrainDemo2 is generated by code** (`sample_terrain2`, ignored writer + check + rules test + headless
     play): 512 m, 513² heightmap of ridged fBm mountains around a meadow with terraced cliffs, four
     rule-driven layers (Meadow, Scree, Cliff with height + normal map, Snow on a height band), meadow-only
     grass, Color Harmony on (0.45). Tileable textures are generated into `Assets/Textures/Terrain/`. An
     ignored `write_terrain_demo2_variants` writes before/after projects for the screenshots.
387. **Terrain merge with fog, probes, AO and cascades:** the terrain and grass shaders get the same
     includes as meshes (soft shadows, probes, screen-space), so `shade` keeps fog, probe reflections, AO and
     contact shadows. `terrain_surface` sets `Surface.draw` to the terrain's draw slot; grass has no draw of
     its own, so its uniform carries the terrain's slot (probe list) in a new `draw` lane. Terrain shadow
     casters use the vertex-pulling `terrain_shadow` pipeline in every cascade and local layer. The AO depth +
     normal prepass draws terrains through a `vs_terrain_prepass` entry (ao_prepass.wgsl + terrain_vertex.wgsl
     + ao_prepass_terrain.wgsl) with the terrain group at 3, instead of the old vertex-buffer prepass variant.
     The terrain fragment stage samples 10 textures (fog volume, two shadow arrays, probe atlas and BRDF LUT,
     splat map, three layer arrays, normals; group 2 stays empty), under the default 16.

388. **Per-object material overrides live in the draw data, not in materials.** `MeshRenderer.overrides`
     maps a Materials element to `MaterialOverrides` (tiling, offset, UV rotation, colour tint, smoothness /
     metallic / emission multipliers), each an `Option` left out of files when unset (renderers without
     overrides save byte-for-byte as before). The gather applies them to that draw's own values, which go
     into its `draws[]` entry; textures and shader variant never change, so the batch key is the same and
     objects sharing a material still draw as one instanced call. Tiling/offset/rotation replace, the rest
     multiply. UV rotation turns about the texture centre before tiling and offset. Terrains (their layers
     already have tiling) and Sprite Renderers (colour, flip) have no override section.
389. **One shared `draws[]` definition.** The WGSL `struct Draw` moved to `shaders/draw.wgsl`, prepended to
     mesh/depth/terrain sources, with the byte layout in a comment; `DrawUniform` grew one vec4 `extra`
     (UV rotation cos/sin, LOD cross-fade, one reserved lane) to 224 bytes. Other per-draw values go at the
     end of both, in that one place. `draw_uv` is written as `uv + (R − I)·p` so no rotation returns the
     exact old coordinates.
390. **Override UI:** a collapsed "Overrides (this object only)" foldout under each Materials element: a
     checkbox per property (switching one on starts from the material's own tiling/offset, or from "no
     change"), overridden rows bold with a ● marker and the count in the header, Reset All. Undo comes from
     the usual component edit; multi-object edits now merge nested objects field by field (one object's
     tiling change doesn't wipe another's colour override), and the mixed-value check also notices fields
     only another target has. Lua: `renderer:SetOverride/ClearOverride/GetOverride(name[, element])`.
391. **Cached gather by comparison, not change events.** Scripts, physics and editor code write components
     directly, so `DrawCache` keeps last frame's inputs and compares: one hierarchy walk with hecs views
     (local transform + parent → world matrix recomputed only below a change, in `world_matrix`'s exact
     operation order), Mesh Filter, Mesh Renderer (now `PartialEq`), outline and LOD level per renderer, and
     each distinct material resolved once per frame (a version bumps only when its values change). The
     output is an `Arc<Vec<MeshDraw>>` patched in place, so `Frame` is unchanged and nothing is copied when
     nothing changed. Lights, cameras, post volumes and terrains no longer walk the whole scene
     (`Scene::active_with::<T>()` queries and sorts the few objects into Hierarchy order).
     `mesh_draws_culled` is a one-off cache, so cached and uncached output can't drift apart.
392. **Parallel renderer prep with rayon.** Per-draw LOD state, world bounds, effective shader variants,
     batch keys and `draws[]` entries are computed in parallel above 512 draws; every shadow map is culled
     and batched on its own thread with its own batcher, then the slot runs are concatenated in map order
     (identical output to the sequential path).
393. **Static batching at gather time (Play mode and the player).** Static objects without a LOD Group are
     grouped by resolved material + shadow flags + a 16 m chunk of their bounds centre and merged into one
     world-space mesh per group (registered as a `runtime://` model so every path finds it as a mesh asset);
     a chunk is still culled on its own. Merging happens again only when a member changes. The Scene view
     keeps objects separate for picking/outlines. CPU-transformed vertices could round differently from the
     GPU (the check allows ≤0.1% pixels; it measured 0).
394. **GPU culling is two-phase (no popping):** draws visible last frame are drawn first, their depth is
     reduced to a Hi-Z pyramid (farthest of 2×2 pixels × all MSAA samples, then 2×2 texels per level), then
     every candidate is tested against the frustum and the pyramid with this frame's camera and the newly
     visible ones are drawn. A compute pass writes each batch's visible instances in order (one thread per
     batch, so the order is deterministic) and their count into `DrawIndexedIndirectArgs`. Batches are
     sorted by shader/textures/vertex buffer; primitives now share one vertex/index buffer, so neighbouring
     batches of different primitives go out as one `multi_draw_indexed_indirect` (wgpu loops where the API
     lacks it). Needs `INDIRECT_FIRST_INSTANCE` (requested with the timestamp feature); otherwise, or with
     the option off, the CPU path runs. Shadows and transparent draws stay CPU-culled. Off by default:
     StressDemo has little occlusion and measured slower with it (see progress table).
395. **Depth prepass is optional and off by default.** Opaque draws write depth with the mesh shader's own
     vertex stage (`@invariant` position), then shade with less-equal and no depth writes. It renders
     identically but measured slower on StressDemo (cheap shading, low overdraw). Opaque draws are already
     front-to-back within instanced batches (batches keep the order of their nearest member).
396. **LOD Group:** levels list Mesh Renderer objects (several per level, references remapped by scene files
     and prefabs; unmapped ones dropped). The gather tags each listed renderer's draws with the group's world
     bounds (largest side = size) and its screen-height range; the renderer picks the level per camera
     (`size / (2·distance·tan(fov/2))`, × `RenderOptions::lod_bias`, orthographic: `size / (2·ortho size)`),
     and shadows use the camera's choice. Cross Fade dithers both levels inside a band of `fade_width` above
     each threshold with complementary 4×4 patterns; a fading opaque level draws with the cutout variant so
     plain opaque shaders never contain a discard. No fade for unlit/transparent levels, no "Animate Cross-
     fading" speed-tree mode, bounds are recomputed instead of a stored reference point/size.
397. **Inspector LOD bar:** coloured segments from 100% to Culled, drag a divider to move a threshold (kept
     decreasing), click a segment to edit its renderer list (add from the group's descendants, Fill from
     Children), a white marker for where the Scene view camera currently is. +/− Level, Fade Mode/Width.
398. **Overrides + performance merge:** `draw.wgsl` (224-byte `Draw`) is prepended to every shader that
     reads draws, the AO prepass included (its own copy of the old 208-byte struct was removed), and the
     terrain, terrain shadow and grass sources. The parallel frame lists build one shadow list per cascade
     and local layer (`ShadowPlan::matrices`); the probes keep their per-draw lists, now fed the cached
     gather's bounds. Two prepasses stay separate: the optional main depth prepass (Opaque depth into the
     MSAA target, then Equal tests) and the AO / contact-shadow prepass (its own single-sample normal +
     depth targets that the compute passes read). They don't share targets (sample count and the normal
     output differ), so unifying them would cost an MSAA resolve. GPU culling is off while AO or contact
     shadows run, because their prepass draws the CPU-culled opaque batches. The main pass keeps its depth
     for the sun shafts and between GPU-culling phases. Quality ▸ LOD Bias feeds `RenderOptions::lod_bias`
     in `Renderer::set_quality`.

## 2026-11-04 — World Builder (procedural world generation)

399. **One window, four sections:** Window ▸ Terrain ▸ World Builder is a single on-demand tab (docks by the
     Inspector) with collapsible sections Base Heightmap (erosion inside it), Stamps & Areas, Biomes and
     Generate. No extra tabs or sub-windows; stamps can also be edited from the Inspector.
400. **The recipe lives in the scene:** settings are a `World Builder` component on a "World Builder" object;
     every stamp/area is a child object with a `World Stamp` component (its transform gives the centre,
     height, yaw and x/z scale). Generated water and scatter go under its "Generated" child and are replaced
     on every Generate. No separate `.worldbuilder` file: saving the scene saves the recipe, and scene undo
     covers edits to it.
401. **Deterministic pipeline** (`world_gen::generate::run`): base → hydraulic → thermal → stamps (hierarchy
     order) → water → classification → splat → scatter. Pure data in and out on a worker thread with a
     shared `Progress` (stage, fraction, Cancel). All randomness comes from the base seed (stamps add their
     index).
402. **Noise:** own seeded gradient noise, fBm / ridged (weighted octaves) / billow, per-octave rotation and
     two-field domain warping. The generators (Rolling Hills, Mountains, Plateaus/Mesas, Canyons, Islands,
     Dunes) are short shaping functions over it.
403. **Erosion on the CPU with std threads (no new dependency):** hydraulic droplets run in batches over 16
     fixed chunks. Each chunk sees the batch's snapshot plus its own changes, and the changes are summed in a
     fixed order, so results don't depend on the core count. Sediment still carried when a droplet stops is
     deposited, so mass is conserved. Thermal erosion uses a two-pass gather form (parallel and conserving).
     Heights are in cell units while eroding, so the constants don't depend on scale. At 1025²: about 1.5 s
     hydraulic + 0.4 s thermal (release, 16 threads), so no wgpu compute pass was needed.
404. **Stamps:**
     - Mountain/Hill: Cone, Ridge (along local X), Volcano (with a crater) or Mesa, blended with
       Add/Max/Min/Replace. Max, Min and Replace use the object's height as the base.
     - Flatten: flattens to the object's height, or to the ground height at generation time (Sample Ground),
       with edge falloff.
     - River: Catmull-Rom curve through local points. The bed never rises downstream; the channel is rounded
       with a bank falloff, and a water strip is optional.
     - Water Body: Lake = 4-neighbour flood fill below the object's height, starting from its position;
       Sea = everything below that height.
     - Rock Area.
     - Biome Zone: a circle or a polygon.
405. **Water is a basic transparent plane** (`water::spawn_water`, Plane primitive + `Water.mat`): one per
     lake (over the fill's bounds), one sea plane (8× the terrain) and strips along rivers. That one function
     is the swap point for the dedicated ocean system. The flood fill still drives the wet map that shore
     classification uses.
406. **Biomes are `.biome` JSON assets** in `Assets/WorldBuilder/`. Each holds a texture + tiling per feature
     (Flat, Slope, Cliff, Shore/Underwater, Peak/Snow), thresholds (slope and cliff angles, peak line, shore
     width), scatter items and a colour tone. There are six presets (Forest, Desert, Grassland, Snowy
     Mountains, Beach, Rocky Badlands) using seven generated tileable textures. Edits save the biome asset
     directly; they are not part of scene undo.
407. **Classification per splat texel:** slope, height, curvature (ridges expose rock earlier), chamfer
     distance to water and height above the nearest water level. Weights are taken in the order Cliff,
     Shore, Peak, Slope, Flat. Biome weights come from zones in hierarchy order (later zones paint over
     earlier ones), sampled at a noise-jittered point (Edge Breakup) so boundaries look natural.
408. **Own splat painter behind `paint::SplatPainter`** (`BiomeRules`). Each distinct texture across the biomes
     becomes a terrain layer. With more than four (the terrain's limit), each used biome's Flat texture is
     kept first, then the most used of the rest; a dropped texture's weight moves to its biome's Flat layer.
     This trait is the swap point for the terrain agent's rule-based
     auto-painter at merge time.
409. **Scatter:** one jittered candidate per min-spacing cell, kept with probability biome weight × density
     under the item's rules: hard slope and height limits, dry ground only, Rock Areas boost rocks and
     suppress plants. A candidate is rejected when it is closer than the item's spacing to another instance
     (spatial hash) or overlaps another non-grass object. Each item is a primitive shape (tree = trunk +
     crown, rock, bush, grass tuft, cactus) or a prefab; colours are tinted 15% by the biome tone.
410. **Scatter storage:**
     - Auto: objects, switching to compact above 20 000 instances.
     - Objects: one object per instance with shared materials, so automatic instancing batches them.
     - Compact: one `Scatter` component per item, 14 bytes per instance (16-bit positions over its bounds,
       yaw, scale, up) stored as base64. `render_scene::mesh_draws` expands it into mesh draws (editor and
       player), so the instancing batcher still draws them.

     Prefab items always spawn objects. Generated materials go to `Assets/WorldBuilder/Materials/`.
411. **Live preview:** while the window is open, any change to settings or stamps (moving a stamp counts)
     regenerates at 129² after 350 ms without changes, with no scatter and no saving. The data from before the
     first preview is kept: Revert Preview restores it, and the next Generate uses it as the undo state. A full
     Generate (or Heights / Textures / Scatter Only) saves the terrain asset and is one undo step covering
     both terrain and scene.
412. **Gizmos:** while the window is open or a stamp is selected, stamp outlines are draped on the terrain
     (radius, falloff ring, river centre, edges and banks, zone polygon, water-level ring). Dot handles drag
     the radius, falloff, river points and polygon vertices. Moving, rotating and scaling use the normal
     transform tools.
413. **WorldBuilderDemo is generated by code** (`sample_world_builder`: writer + check + play test). It is a
     600 m island (513² heightmap, 256² splat) with a volcano (Replace) holding a crater lake, a river to the
     south coast, a village plateau (Sample Ground), a rock area, forest by default with snowy-peak and beach
     zones, the sea, and compact scatter.
414. **Differences from tools like Gaia / MicroVerse** (only their behaviour was borrowed): no node graph and no
     library of heightmap-image stamps (stamps are analytic shapes), no GPU erosion, no more than four splat
     layers, no spline roads, no streaming or tiling across several terrains, and water is a simple plane.
415. **World Builder merge:** Scatter draws join the DrawCache output after the sprites (rebuilt every
     update like them, filtered by the view's culling mask), since `mesh_draws_culled` is now a one-off
     DrawCache. `MISTY_VIEW` takes either `x,y,z,pitch,yaw` (five numbers) or an object name. World
     Builder keeps its own `BiomeRules` splat painter: the terrain Auto Paint rules are per terrain layer
     (height / slope / spread / breakup over one terrain), while the World Builder weights textures per
     biome × feature and blends biomes with their masks before folding to four layers. Mapping one onto the
     other is not a drop-in swap, so `SplatPainter` stays the seam for a later rule-based painter.
     Generated terrains are ordinary `TerrainData` (513² heights), so they render through the
     vertex-pulling, texture-array terrain renderer unchanged.
416. **GPU tests run one at a time:** every headless test made its own device, and a dozen of them
     creating devices and rendering at once deadlocked inside the driver now and then (all GPU tests then
     waited forever with no CPU use; it showed up after the merges roughly doubled the GPU tests). The
     engine and editor `headless()` helpers take a process-wide lock that the calling test thread holds
     until it ends, so GPU tests serialise while the CPU tests stay parallel. The World Builder panel tests
     use absolute terrain paths, since other tests move the process-wide project root while their
     generation job runs.

417. **Custom shaders are WGSL files with a comment header:** `.shader` assets start with `// @shader`, `// @blend`,
     `// @cull`, `// @zwrite`, `// @queue` and `// @property NAME TYPE ["Label"] = DEFAULT // tooltip` lines. Because
     the header lines are WGSL comments, the file compiles as written. This replaces ShaderLab's `Properties {}`
     block and needs no second language. Property types are float, range, color (linear in the shader, sRGB in the
     Inspector), vector and texture2d (defaults white/black/grey/normal).
418. **Surface shaders reuse the lit mesh shader:** the generated module is common.wgsl + mesh.wgsl + a prelude +
     generated bindings + the user's file + entry points. `fs_custom` fills the engine's `Surface` from the user's
     `SurfaceOutput` and calls `shade()`, so a custom shader picks up any improvement to the lighting (shadows, fog,
     ambient, probes, AO). Unlit shaders return the final colour from `fn fragment`. An optional
     `fn vertex(VertexData)` displaces in object space. Shadows use the built-in caster (undisplaced, opaque
     only). Custom bindings live at group 2 bindings 16+ and group 3 (`globals`, scene copy), so they never
     collide with the built-in material's bindings.
419. **Errors and fallback:** naga parses and validates the wrapped module before wgpu sees it, and pipeline creation
     runs inside a validation error scope. Error spans are mapped back to lines of the user's file; errors in the
     wrapper are reported without a line, with a hint about the expected function. A broken or missing shader draws
     with a magenta unlit shader. Errors go to the Console as `Shader error in <path>:<line>:<col>: …`, which the
     Console links to the Code Editor. The Code Editor highlights WGSL and shows naga errors as squiggles.
420. **Hot reload:** a process-wide shader library caches assets by path and re-reads changed files (mtime, polled
     twice a second in Edit mode). While you type, the Code Editor pushes the buffer into the library once it
     compiles, so the scene updates live; saving reloads it. Each load bumps a generation number, and the renderer
     recompiles when that number changes.
421. **Materials store values by name:** `.mat` gets `"properties": {"_Speed": 2.0, "_Color": [..], "_MainTex": "Assets/.."}`
     (left out when empty, so old files re-save unchanged). Values that do not fit the declared type fall back to
     the default. The shader's blend/cull/queue map onto MaterialParams (Fade / Transparent / double sided /
     `render_queue()`), so draw order, shadows and batching work as for built-ins. A uniform + texture bind group is
     shared by every material with the same shader and the same values, and the batch key is the custom pipeline +
     group + mesh.
422. **Scene colour and depth for the transparent queue:** when a frame has water, or a queue≥3000 custom shader that
     reads the scene, the main pass splits after opaque geometry, terrain and the grid. The resolved HDR colour is
     copied, and MSAA depth sample 0 is resolved into an R32Float texture by a fullscreen pass. Water and
     transparents then draw in a second pass that loads the MSAA targets. Frames without water pay nothing.
423. **Ocean = Gerstner + FFT, one shared model on GPU and CPU:** 8 Gerstner waves (4 swell from Swell
     Height/Length/Direction, 4 long wind waves) plus a 256² FFT ocean with a JONSWAP or Phillips spectrum. The FFT is
     scaled so that Hs = 0.21·V²/g × Wave Height, and its tile is about two peak wavelengths (48–900 m). The CPU
     builds the identical initial spectrum (deterministic hash Gaussians), so `Water.GetHeight` and buoyancy evaluate
     the same Gerstner waves plus the FFT's 32² lowest modes, with a fixed-point inverse of the sideways motion. Only
     the finest ripples are missing on the CPU. All surfaces share the first surface's FFT simulation (scaled to their
     own wave height); their Gerstner waves, colours and foam stay their own.
424. **FFT on the GPU:** one compute pass animates h0, runs radix-2 row and column inverse FFTs in workgroup memory
     (4 complex signals carry height, x/z displacement, slopes and Jacobian terms), then writes rgba16f
     displacement and slope textures with 8 mips. Foam builds where the Jacobian drops below a threshold that rises
     with Crest Foam, fades over about 2.5 s, and is stored in a persistent buffer. The FFT runs once per new time
     (several views per frame share it).
425. **Ocean geometry is a camera-centred clipmap:** 9 square rings (0.5 m cells doubling to 128 m, ±4 km) plus a
     skirt to 60 km, snapped to the camera in 4 m steps. Vertices near a ring's edge morph onto the next ring's grid,
     so there are no cracks. Short Gerstner waves fade per cell size, FFT samples use the matching mip, and
     displacement flattens towards 3.5 km so the horizon stays calm. Lakes and rivers use one 160² unit grid scaled
     to their size, with waves fading towards the banks. Rivers turn their waves along the flow and scroll the FFT
     with Flow Speed.
426. **Water shading:** Fresnel reflection of the procedural sky, plus a 24-step screen-space march against the depth
     copy (Screen Space Reflections toggle). There is no probe reflection yet; it hooks in when probes land.
     Refraction distorts the scene copy by the normal, rejects samples in front of the water, and absorbs along
     the path (red first) over Clarity metres towards a shallow↔deep body colour lit by ambient and sun. SSS adds
     light through thin crests towards the sun, and the sun glitter is GGX, roughened with distance. Shore foam
     comes from the depth difference and crest foam from the FFT Jacobian and Gerstner crests, broken up with value
     noise. Caustics are procedural moving Voronoi edges, projected along the sun onto the underwater scene point
     and faded with depth. Water writes depth, receives the sun's shadow and takes fog.
427. **Underwater view:** when the camera is below a surface (CPU height query at the camera), a fullscreen pass before
     the water redraws the opaque scene with wobble, caustics, distance fog in the underwater colour and light falling
     off with depth. The underside of the surface shows Snell's window, with total internal reflection outside it.
428. **Floater = eight corner probes:** each corner of a local box pushes up by Buoyancy × weight / 8 × its submerged
     fraction (half at the surface). This keeps working when the box turns over; bottom-only probes made flipped
     crates float upside down and too deep. Linear and angular water drag act relative to the river flow. It runs
     inside `Physics::step`, before the rapier step, and uses the same `clock` time as rendering
     (`Time.time` while playing).
429. **Simple controls over raw parameters:** the Water Surface Inspector has the type, size, a Calm ↔ Stormy slider
     (sets wind, height, chop, swell, foam and ripples together) and preset buttons (Tropical, Ocean, Stormy, Lake,
     Murky River), followed by collapsible sections with tooltips. GameObject ▸ Water ▸ Ocean / Lake / River
     creates surfaces. `water::spawn_water(scene, name, kind, position, size)`, `water::height_at` and
     `WaterSurface` / `Floater` form the public API for other systems, such as the world builder's lakes.
430. **WaterDemo and ShaderDemo are generated by code** (`sample_water`: ignored writer, check, headless play test that
     the crates float). WaterDemo has an ocean at sunset, a sand-dome island with rocks, a palm, a hologram beacon,
     seven floating crates and an orbiting camera. ShaderDemo shows Hologram (lit, alpha), Wobble (vertex), Scroll
     Gradient (unlit) and Force Field (reads the scene copy).
431. **Custom shaders + ocean merge:** `.shader` assets and the water are appended to the full
     `MESH_SRC` (draw.wgsl, mesh.wgsl, soft shadows, probes, screen-space) after `COMMON` with the fog
     include, so `shade(world, Surface)` keeps every effect. The main pass now has both splits: GPU
     culling's two phases around the Hi-Z build, and the opaque / transparent split around the scene copy
     (water, scene-reading shaders); the copy happens only after the last culling phase, and the depth is
     kept when the sun shafts need it. Draw prep keys `.shader` materials by their own pipeline and group;
     GPU culling and the main depth prepass step aside while any `.shader` material is drawn (they use the
     built-in variants, and custom vertex code may move vertices). World Builder water now spawns
     `WaterSurface`s through `water::spawn_water`: the sea as the Ocean, lakes as Lake rectangles, river
     strips as Rivers flowing along their axis (the old plane + Water.mat is gone from new generations).

## 2026-11-04 — Light halos and lens flares

432. **Halo is a section of the Light, Lens Flare is its own component:** `Light.halo` (`LightHalo`: Enabled, Size,
     Intensity, Color = `None` for the light's colour, Softness, Fog Response) is left out of scene files while
     untouched, so old scenes re-save byte-identical. Lens Flare is a component (category Rendering) with Intensity,
     Scale, Tint, Ring / Streak toggles and 0–8 Ghosts; it takes its colour from a Light on the same object (white
     without one). No flare asset: the elements are procedural, so there is nothing to author, and a handful of
     options covers the useful range.
433. **Glow sources travel in `Environment::glows`** (gathered by `render_scene::glow_sources`, like the Fog Volumes),
     so neither `LightData` nor `Frame` changed and the ~25 existing frame / light constructors stay as they are.
434. **One additive pass after the main pass** (`render/glow`), before the sun shafts and bloom, writing both the
     resolved HDR image and the glow (emission) image, so bloom spreads the halos. Halos are sphere-facing quads with
     the radius in metres; flare elements are screen-space quads on the line from the light through the screen
     centre (axis position 1 = at the light, negative = mirrored past the centre).
435. **Occlusion in the vertex shader:** each instance reads the main pass's MSAA depth (sample 0) at 13 Vogel-disc
     taps around the light (a fifth of the halo radius; a fixed 1.5% of the screen for flares) and scales its
     colour by the visible share, so lights fade smoothly as objects pass in front instead of popping. A tap only
     counts as covering when the scene is closer than the light by more than a bias (35% of the halo radius,
     0.1–1 m; 0.25 m for flares), so the lamp's own head and bulb don't hide it. Halos additionally do a per-pixel
     soft-particle fade against geometry just in front of their plane. The main pass keeps its depth whenever
     glows draw (as it does for sun shafts).
436. **Fog response on the CPU from the same inputs as the fog:** extinction at the light = the Fog override's height
     fog + Fog Volumes (volumetric mode only, like the froxels) + the Lighting window's fog. Fogginess
     `g = 1 − exp(−σ · 10 m · response)`; the halo radius grows by up to 1.5×, brightness goes from 0.35× (clear
     air: a subtle glow) to 1.75×, and softness rises toward 1. The fragment shader also dims by the fog
     transmittance between the eye and the light (`vfog_eval`), and the fog debug views hide glows. Reading the
     froxel texture would need a readback or vertex-stage fog bindings for the same answer.
437. **Spot lights glow only when looking into the cone:** facing = smoothstep from 12° outside the half angle to
     60% of it (dot of the light direction and the direction to the eye), on halos and flares alike. Directional
     lights get no halo (the sky's sun disc does that) but can carry a Lens Flare, anchored at half the far plane
     opposite their direction.
438. **Quality:** halos always draw (they are a few quads); `QualityLevel::lens_flares` (Low = off, others on,
     Project Settings ▸ Quality ▸ Lens Flares) gates the flares. Caps: 256 halos and 32 flaring lights per render,
     nearest first. Flares fade out over the last 20% of the screen and past its edge.
439. **Scene view:** glows follow the existing effect toggles — they draw when the effects master toggle,
     Post Processing and Lighting are all on — and never in Wireframe. The Light Inspector gets a collapsible
     "Halo" section (open while the halo is on, rows greyed out while disabled; point and spot lights only).
440. **LightGlowDemo is generated by code** (`sample_light_glow`, ignored writer + generator check): a foggy night
     street (solid dark sky, faint moon, volumetric fog with a ground-mist Fog Volume), six street lamps alternating
     warm sodium / cool LED with halos, a doorway spot light shining down, and a floodlight at the far end facing the
     camera with a halo and the scene's one Lens Flare.

## 2026-11-04 — HDRI skies

441. **HDRI import through the image crate:** the workspace `image` dependency gains the `hdr` (built in) and `exr`
     (pure Rust `exr` crate) features. `hdri::HdrImage` decodes both to linear float RGB, turns NaN / negative
     texels into 0, clamps at 60 000 (half-float textures top out at 65 504) and box-filters images wider than
     4096 by powers of two. HDR images are a separate list (`hdri::EXTENSIONS`, `AssetDatabase::hdris`), not
     material textures; the Project window treats them as textures for icons, thumbnails and `t:Texture`.
442. **Equirect convention:** u = 0.5 + atan2(x, z) / 2π, v = acos(y) / π, so the image centre faces +Z (the
     default camera's forward) and +X is to its right; the same formulas run on the CPU (`hdri/equirect.rs`) and
     the GPU (`convert.wgsl`). Rotation turns the sky about +Y (`hdri::rotation`); lookups undo it.
443. **Our own Radiance writer** (`hdri::rgbe`): shared-exponent RGBE with per-channel run-length scanlines (runs
     ≥ 3, literal chunks ≤ 128), flat scanlines for widths outside 8..32767. The sample HDRI is written with it;
     the test reads files back through the image crate's decoder.
444. **Settings:** `SkyboxKind::Hdri` plus `Skybox::hdri: HdriSettings` (texture path, Rotation in degrees,
     Exposure in EV = ×2^EV, sRGB Tint). The block is left out of the file while default, so every scene saved
     before stays byte-identical and loads unchanged. A missing or broken file shows its error in the Lighting
     window and the sky falls back to the procedural one.
445. **Two GPU cubes per HDRI, built once per file:** a background cube (face = width / 4 rounded to a power of
     two, 64..512, 2 × 2 equirect taps per texel, full box mip chain) and a specular cube (128², seven levels with
     the probes' roughness mapping, GGX filtered importance sampling with 512 samples reading the background mip
     that matches each sample's solid angle; level 0 is the background mip of the same size). Compute passes with
     automatic layouts, one submission, keyed by the decoded image's identity. Rotation, Exposure and Tint are
     applied at lookup, so moving their sliders never rebuilds anything.
446. **Diffuse from SH9 on the CPU:** projected from a 128 × 64 copy with solid-angle weights and cosine-convolved
     (`probes::sh`); rotation re-projects that copy (cached per angle) instead of rotating SH coefficients.
     Exposure, tint and the ambient Intensity Multiplier are folded in on the CPU, the GPU evaluates world-space
     SH. With Source = Gradient or Color, the HDRI still draws and reflects but the ambient is those colours,
     projected onto SH so the same shader path serves all sources.
447. **The sky lives in the view group** (group 0, bindings 4–7: `SkyEnv` uniform, background cube, specular cube,
     sampler) so the skybox, meshes, terrain, grass, water and custom shaders all see it through `sky_color` /
     `sky_hdri_*` in common.wgsl. A separate uniform because the view uniform must stay within the 512-byte
     shadow-view stride. Sampled textures per fragment stage stay at 12 or fewer.
448. **The HDRI is the probe at infinity:** `probe_lighting` blends the draw's probes as before, then the HDRI fills
     the weight they leave (prefiltered cube + SH, split-sum like a probe). Probes that aren't baked have no data,
     so their area falls back to the sky; bakes and Realtime captures render the skybox, so they capture the HDRI.
     The probe debug view shows probe coverage only. The procedural sky keeps its old reflection/ambient path,
     so existing scenes render exactly as before.
449. **Sun handling:** an HDRI skybox draws no procedural disc (the image has its own sun); texels above 8 feed the
     bloom glow target. Lighting ▸ Align Sun to HDRI finds the brightest texel of the 128 × 64 copy, refines it by
     the luminance-weighted centre of its 3 × 3 neighbourhood, applies Rotation and turns the brightest enabled
     Directional Light (in its parent's space) to shine from there, as one undo step. The renderer's sun is
     still the brightest Directional Light (volumetric fog, shafts, water glitter, shadows).
450. **Fog and water use the HDRI:** volumetric fog's ambient comes from `Environment::ambient()`, which with an
     HDRI evaluates its SH (up / equator / down); the water's ambient is the SH and its reflection the specular
     cube at the surface's smoothness. Classic fog keeps its own colour, as Unity's does.
451. **Editor:** Lighting ▸ Environment ▸ Skybox = HDRI shows an image slot (picker of the project's HDR files
     plus drag and drop from the Project window), a tonemapped preview, Rotation (0–360°), Exposure (±10 EV),
     Tint and Align Sun to HDRI. Dropping an HDR file anywhere on the Scene view sets Skybox = HDRI with that
     image (one undo step, like dropping a material on the sky). Thumbnails are tonemapped with auto exposure
     (log-average luminance → middle grey) and Reinhard. `MISTY_LIGHTING_PAGE=Environment` opens that page for
     screenshots.
452. **HdriSkyDemo is generated by code** (`sample_hdri`): the 2048 × 1024 "Sunset Clouds.hdr" (gradient sky warm
     towards a sun 7° up, a 0.55° sun disc at 4000, its glow, fbm clouds lit from the sun's side, dark ground)
     written by our writer, and a scene with chrome → rough steel, glossy → matte white and gold spheres on a grey
     floor, lit by the HDRI and a soft-shadowed sun aligned with it. The check compares 400 texels of the committed
     image with the generator within RGBE precision rather than bytes (float functions may differ between
     machines), and that its brightest point is the sun.

## 2026-11-04 — Rectangle area lights (LTC)

453. **A fourth light type, `Rectangle`:** the Light component gains Width / Height (local X / Y, metres, default
     1 × 1), Two Sided (off: emits only towards local +Z) and Show Emitter (on). Each new field is left out of scene
     files at its default, so older scenes re-save unchanged. Object scale does not resize the rectangle (like the
     cone of a spot light). `Light::rectangle()` (radiance 4, soft shadows) is what GameObject ▸ Light ▸ Rectangle
     Light creates, turned 90° on X so it starts as a ceiling panel shining down.
454. **Units: colour × intensity is the radiance of the emitting surface.** A surface facing an infinitely large
     rectangle receives colour × intensity, the same as a directional light of that intensity hitting it head on,
     so the diffuse term is albedo × radiance × (cosine-weighted solid angle / π) with no extra π. The visible
     emitter quad has exactly that brightness, so a mirror reflects it at the emitter's own brightness. There is
     no inverse-square factor (the solid angle falls off by itself); Range only windows the light smoothly to
     zero, measured from the rectangle's closest point so wide panels are not cut off near their edges.
455. **Linearly transformed cosines with our own fitted table.** `render/area/ltc_fit.rs` fits, for 64 roughness
     columns (perceptual roughness, α = r² clamped to 0.002 like the mesh shader) × 64 view rows
     (√(1 - cos θ)), the inverse 3×3 matrix [a 0 b; 0 1 0; d 0 e] whose transformed cosine best matches the
     normalised GGX lobe (height-correlated Smith G2). The error is the cubed difference, estimated with 24²
     grid samples from both distributions (balance heuristic), minimised with our Nelder–Mead (two rounds).
     Each column starts from a lobe shrunk around the normal and each next view angle starts from the previous
     cell's f32-rounded result, so the fit is deterministic and any cell can be re-fitted from its stored
     neighbour (`ltc_table_matches_its_generator`). The same pass stores the lobe's integral split by Schlick's
     weight (scale, bias: F₀·scale + bias). The full fit takes ~24 s in release; the result is committed as
     `render/area/ltc_ggx.bin` (128 KB of f32) and regenerated by the ignored test `write_ltc_table`. No
     third-party table is used.
456. **Polygon clipping, not a horizon-sphere approximation:** the shader transforms the four corners, clips the
     polygon to the upper hemisphere (Sutherland–Hodgman, at most five vertices) and sums the exact edge
     integrals (θ/sin θ × the edge normal's z, with a series near θ = 0). That needs no extra table and is exact
     for the transformed cosine. Diffuse is the same integral with the identity matrix. The sign of the result
     says which face is seen: one-sided lights keep max(0, ·), two-sided ones the absolute value. A CPU copy
     (`area/polygon.rs`, `area::integrals`) backs the tests: the diffuse term matches a brute-force form factor
     and the specular term matches brute-force GGX over a rectangle within 12 %.
457. **Table on the GPU as a storage buffer** (mesh group 1, binding 7) with manual bilinear filtering, instead
     of a texture: the terrain pipeline is close to the 16-sampled-textures-per-stage limit, and a storage buffer
     needs no sampler. All lit shaders get it through the shared group: meshes, terrain, grass and custom
     `.shader` materials (they all end in `shade()`), and water calls `rect_light` for glints and body light.
     `LightItem` grew from 80 to 112 bytes (two vec4s: half-width axis + two-sided, half-height axis); the light
     list is still the same 8-light uniform the other types use, so rectangles cost one loop iteration with two
     polygon integrals and two table lookups.
458. **Shadows: a spot-like map from the centre.** A shadowed rectangle takes one local shadow layer with a
     perspective frustum from its centre along its forward axis with a fixed 120° field of view
     (`RECT_SHADOW_FOV`); points further off-axis are left unshadowed (wider frusta waste resolution).
     Soft-shadow (PCSS) source size = half the mean side (¼ × (width + height)), so the penumbra grows with
     the light; the Inspector hides Shadow Radius for rectangles. Two-sided lights only shadow their front.
459. **Volumetric fog: an emitter at the centre** with the rectangle's solid angle, area × facing cosine /
     (d² + area/π) (finite near the light), ÷ π for surface units, and the same range window and shadow map.
460. **Show Emitter draws an emissive quad** inside `Renderer::render`: when a rectangle light has it on, the
     frame's meshes are copied once with black, rough, emissive quads added (emission = colour × intensity,
     double-sided when the light is, no shadows). Doing it in the renderer covers the Game view, the Scene view,
     the player and reflection-probe captures with no change to the draw caches; the copy only happens when
     emitters exist.
461. **Editor:** Width, Height, Two Sided and Show Emitter rows appear in the Light Inspector only for
     Rectangle lights. The selected light's gizmo is its outline, short rays from the corners on the lit
     side(s) and an arrow along its forward axis. Four dots on the edge midpoints drag the edges in the light's
     plane; the rectangle stays centred on the light, so the opposite edge moves too (one undo step,
     "Light Size"), like the familiar area-light handles.
462. **AreaLightDemo** (`sample_area_lights`): a dim 8 × 3 × 10 m room (no sun, faint colour ambient, dim
     reflections, solid dark sky) with a warm 3.6 × 0.3 m ceiling strip, a cool 0.6 × 1.8 m wall panel and a
     magenta 0.7 m two-sided square, over a glossy floor with chrome, glossy and rough props.
463. **Clipping without indexed arrays:** the shader clips edge by edge (each edge adds its part above the horizon,
     remembering where the outline leaves and re-enters it, then the closing horizon edge is added) instead of
     building the clipped polygon in a local array and looping over it. Dynamically indexed local arrays spill to
     slow memory on many GPUs; the first version cost ~1 ms per full-screen light at 1080p, this one ~0.22 ms.
     Points behind a one-sided light return before any integral. The CPU reference (tests) keeps the array version.
464. **Merging rectangle lights with halos:** the Light Halo (decision 432) stays a point / spot feature: rectangle
     lights draw no halo sprite and hide the Halo section, because their visible shape is the emitter quad (a
     sphere-shaped glow would contradict it). Lens Flares still work on any light. The LTC table keeps mesh group 1
     binding 7 (free on main: glow is its own pass with its own groups).

## 2026-11-04 — Volumetric clouds

465. **Volumetric clouds = one raymarched layer on a curved planet:** clouds live between Altitude and Altitude +
     Thickness on a 6360 km sphere whose top is the world's y = 0 under the camera, so the layer bends below the
     horizon like the real sky. All sphere tests are written relative to the ground point under the eye (`local()`
     in cloud_common.wgsl) with the stable quadratic, because R² in f32 has no precision left for metres. Density is
     the classic layered model: a 2D weather map (coverage, cloud type) sets the local coverage (weather × 0.9 around
     0.5 plus 1.15 × Coverage, faded in below 0.25 so 0 means a clear sky); a height profile (rounded base, the type
     sets the top) shapes a 128³ Perlin-Worley + Worley fBm base noise that the coverage carves; a 32³ Worley detail
     erodes its edges (wispy at the base, billowy above).
466. **Noise is generated on the GPU, blue noise on the CPU:** the base (128³), detail (32³) and weather (512², 36 km
     tile) textures are tiling rgba8 made by one compute submit (cloud_noise.wgsl) the first time clouds are drawn,
     so no binary assets ship. The 64² blue-noise tile comes from a small void-and-cluster on the CPU (cached per
     process) instead of a downloaded texture.
467. **Few controls:** the override has Enable, a Preset menu (Sparse, Cloudy, Overcast, Stormy; it shows "Custom"
     once values were changed), then four small groups: Shape (Coverage, Density, Fluffy ↔ Sharp), Layer (Altitude,
     Thickness), Wind (Direction, Speed) and Lighting (Silver Lining, Ambient, Cast Shadows, Shadow Opacity).
     Fluffy ↔ Sharp moves two internals together (detail erosion 0.55 → 0.22 and a 1 → 3.5× edge density ramp).
     Presets only set coverage, density, shape and layer, so wind and lighting choices survive a preset change; the
     default override is the Cloudy preset. Everything else (planet size, noise tiles, extinction scale, step counts)
     is a constant or comes from Quality.
468. **Lighting:** each sample marches 4–6 quadratically growing steps towards the sun (the first three with detail),
     then sums three multiple-scattering octaves (energy × 0.5, extinction × 0.3 and phase anisotropy × 0.5 per
     octave) of a two-lobe Henyey-Greenstein phase (forward g 0.8 for the silver lining, back g −0.2; Silver Lining
     sets the mix), with a "powder" darkening of sun-facing edges. Sky ambient goes from the horizon colour (below)
     to the zenith (above) by height in the layer, divided by 1 + ½ × an estimate of the cloud above the sample
     (extinction × local coverage × the rest of the layer × its own density), so thick, dense and stormy layers get
     dark, structured bases instead of a flat ambient-lit sheet. Steps are integrated energy-conservingly. The sun
     colour follows the sky's sunset warming. Distant clouds fade into the average horizon sky colour (28 km half
     distance), and density fades out between 36 and 60 km so the horizon doesn't alias.
469. **Reduced resolution + temporal, depth-independent:** the trace runs at ¼ (Low/Medium) or ½ (High) of the target
     per axis with a blue-noise start offset (animated by the golden ratio per frame). It does not read the scene
     depth, so upsampling is plain bilinear with no edge artefacts. A temporal pass reprojects each texel by its
     cloud distance into last frame's image, clips the history to the current 3×3 mean ± 1.5σ and blends 90%
     history. Both images are two-layer arrays: (light, transmittance) and (distance in km).
470. **Clouds are folded into the fog functions:** `vfog_eval(world)` / `vfog_sky(dir)` (fog_apply.wgsl, used by every
     forward shader) now return fog over clouds: the sky gets the clouds behind the fog, and every surface hides the
     clouds behind it by comparing its distance with the clouds' transmittance-weighted distance (softly over the
     last 30%). So mountains standing in clouds and views from above the layer work with no per-shader changes,
     gizmo lines stay on top, and emission / bloom dims behind clouds. The view group gets three bindings (cloud
     image array, cloud shadow map, `CloudApply` uniform), so every pipeline has two more sampled textures; it is
     rebuilt each render while clouds are on because the history ping-pongs, and once when they turn off.
471. **Cloud shadows = a top-down transmittance map:** 128² / 256² / 512² rgba8 over 8 / 12 / 16 km of the cloud
     base plane, centred on the camera's spot on the base (along the sun) and snapped to whole texels. Each texel
     marches 8–16 steps up through the layer along the sun. Lookups slide the point along the sun onto the base
     plane (sun height clamped to 0.08), fade out at the map edge and mix by Shadow Opacity. The sun is shaded by it
     on meshes, terrain, grass and water (`shade`, `water_sun_shadow`; every directional light, since only the sun
     is expected), in the froxel fog (two new froxel bindings: god rays through cloud gaps) and in the analytic fog
     (looked up at the eye). Sun shafts need nothing: their prefilter reads the sky, which now has the clouds in it.
472. **Quality ▸ Volumetric Clouds tier:** Off / Low (¼ res, 40 steps, 4 light steps, 128² shadows) / Medium (¼, 64,
     5, 256²) / High (½, 96, 6, 512²). The default levels use Low, Medium, Medium (the High level) and High (Ultra),
     so the default editor level stays on the Medium tier. Clouds are also skipped in orthographic views, without
     the skybox, in wireframe and in probe captures (which use the default post settings). The clock is the
     renderer's own (like the fog noise), so clouds drift in Edit mode too.
473. **Scene view and sample:** the Scene view effects dropdown gets a Clouds toggle next to Fog (the ☁ button turns it
     off with the other effects). CloudsDemo (sample_clouds.rs) is a 4 km meadow valley between ridges with a low
     sun (24°) ahead-right of the camera, Cloudy clouds at 45% coverage and density 0.75 drifting across the view,
     light volumetric haze and sun shafts; it reuses TerrainDemo2's generated textures.
474. **Measured cost** (RTX 4060, Vulkan, 1920×1080, release, `clouds_benchmark`, whole-render GPU time minus the
     same frame without clouds, median of 60 frames, camera 12° above the horizon): Cloudy 0.47 / 0.72 / 2.33 ms and
     Overcast 0.19 / 0.30 / 1.17 ms on Low / Medium / High. Medium stays well under the 2 ms target, so it keeps
     quarter resolution; High (half resolution) is for Ultra.

## 2026-11-04 — Round 9 merge (glow, HDRI skies, rectangle lights, clouds)

475. **View group layout after the merge:** 0 view uniform, 1–3 fog volume / sampler / uniform, 4–7 HDRI sky
     (uniform, background cube, specular cube, sampler), 8–10 volumetric clouds (cloud image, cloud shadow map,
     apply uniform). The clouds moved from 4–6 to 8–10 (`clouds::view_layout_entries`, `clouds_apply.wgsl`,
     `volumetrics::view_group`) because the HDRI merged first. `Renderer::rebuild_view_group` is the one place
     the group is built after start-up; the HDRI cube rebuild and the fog volume resize both go through it.
     Forward fragment shaders now see 5 sampled textures in the view group, 4 in the mesh group and up to 5
     material maps (14 for meshes), within the default limit of 16, so no limit was raised. The rectangle
     lights' LTC table stays a storage buffer at mesh group 1 binding 7.
476. **Cloud shadows stay on directional lights:** rectangle (and point / spot) lights are not darkened by the
     clouds — surfaces, water and froxel fog only apply `cloud_shadow` to light type 0.
477. **Clouds under an HDRI sky are lit by its SH:** with Skybox = HDRI the cloud ambient (top and base) comes
     from the HDRI's diffuse SH in world space (up and the four horizontal directions), not from point samples of
     the image, so a bright sun or a feature in the HDRI doesn't tint the whole layer; the distant-haze colour still
     samples the sky's horizon. Procedural / solid skies keep their zenith / horizon colours.
478. **Scene view effects list:** Skybox, Fog, Clouds, Halos & Flares, Post Processing in one dropdown. Halos and
     flares get their own toggle and still need Post Processing and Lighting on.

## 2026-11-04 — Round 10: Global Search (Ctrl+K)

479. **Search palette is a floating modal, not a dock tab:** Ctrl+K opens a 560 px box anchored near the top
     centre (egui `Modal`, light 60-alpha backdrop) with one text field, results grouped under small provider
     headers, and a one-line key hint. Clicking the backdrop, Esc or Ctrl+K again closes it. It is drawn before
     the dock area so Up/Down/Enter/Esc are consumed before any panel sees them, and the global shortcuts are
     skipped while it is open. Inside the Code Editor Ctrl+K stays the chord prefix (Ctrl+K, C …), so search
     opens from everywhere else.
480. **One matching module for every search box:** `search::fuzzy` (in-order subsequence, DP-scored with bonuses
     for word starts, camelCase, consecutive runs and prefixes, gap penalties, exact-name bonus; O(n·m) per name)
     and `search::query` (`t:`, `tag:`, `layer:` tokens; `h: p: m: s:` provider prefixes; `=` calculator). The
     Hierarchy box keeps substring matching (`Mode::Contains`) but now goes through `scene_search` too, so it
     gains `t:Light`, `tag:`, `layer:` filters; the Project window's `SearchFilter` parses with `Query::parse`
     and matches with the same code. The palette uses `Mode::Fuzzy`.
481. **`t:` on objects means "has this component":** compared against registry component names with spaces
     removed and case ignored (`t:Light`, `t:MeshRenderer`, `t:AudioListener`); `t:GameObject` matches all.
     `layer:` takes a layer name or number, `tag:` a tag (case-insensitive). Filters only apply to the Hierarchy
     and Project providers; with a filter in the query, menus / settings / help are not searched.
482. **Project `t:` types gained `AudioMixer` and `Shader`:** `project::type_for_path` is the single type table
     for the Project window and the palette index (previous types unchanged).
483. **Asset index without a new watcher crate:** the editor has no OS file watcher (scripts and textures are
     polled by mtime), so `search::asset_index` follows the same approach: the first build walks `Assets/` on a
     background thread; afterwards, while the palette is open, 24 folders per frame have their modified time
     checked round-robin and only changed folders are re-listed (new sub-folders walked, removed ones dropped).
     Assets ▸ Refresh / Ctrl+R invalidates it for a background rebuild. Results are cached per (query, index
     generation), so typing is the only thing that re-runs providers. 4000 assets query in well under a frame;
     a 5000-object scene too (tests).
484. **Groups ordered by their best match:** results are grouped by provider (Calculator, Hierarchy, Project,
     Menu, Settings, Help) and groups are sorted by their top score (stable, so ties keep that order), so
     "lighting" puts the Lighting window first and "cube" puts the Cube object first. Six results per provider
     without a prefix, fifty with one.
485. **Menu commands come from a parallel table:** `search::menu_items::all` lists the menu bar's commands as
     command-like titles ("Create Cube", "Add Rigidbody", "Lighting", "Default Layout") with the menu path as
     detail and their shortcut; entries with no `Action` (Preferences, Refresh, reflection probes, About) get a
     `MenuRun` variant. The menu bar itself was not rewritten (it builds egui menus inline); a test checks the
     table. Titles match first; the menu path is a fallback with a lower score and no highlight.
486. **Settings and Help providers:** every Project Settings page plus Preferences; page blurbs act as keywords
     ("gravity" → Physics). Help lists the headings of docs/building, scripting, shaders and world_builder
     (embedded with `include_str!`) and opens the guide in the system's Markdown viewer.
487. **Actions:** Enter runs the first action; Alt+Enter or right-click lists all of them in place (Esc returns to
     results). Objects: Select and frame (default), Select, Copy name. Assets: Show in Project (opens the folder,
     selects it, Inspector shows it, focuses the Project tab — the existing `prefabs::select_asset` ping), Open
     (as a double-click: scenes load, scripts open in the Code Editor, folders open), Show in Explorer, Copy path.
     Menu: Run. Settings / Help: Open. Calculator (`=2*3`): Copy result. A removed object only warns.
488. **Recent searches:** a query that led to an action is remembered (8, newest first) in
     `Library/SearchHistory.json`; with an empty box the palette lists them and Enter puts one back in the box.
489. **`MISTY_SEARCH=<text>`** opens the palette with that text for screenshots (the index is built
     synchronously first).
