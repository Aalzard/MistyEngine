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
