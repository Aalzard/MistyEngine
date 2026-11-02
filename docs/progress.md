# Progress

## Step 1 — Window + editor layout ✅

### Done
- Native window (winit) rendered by wgpu through eframe; Unity-like dark theme, OS UI font + symbol fallback.
- Dockable tabs (drag, split, float, close) in the Default layout: Hierarchy | Scene+Game | Inspector,
  Project+Console along the bottom. Window ▸ Layouts ▸ Default and toolbar Layout ▾ restore it.
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
- Hierarchy has no parent/child tree, drag-reparenting, rename (F2) or multi-select. (step 3)
- Inspector edits placeholder data, not real components. (steps 3–4)
- Layouts are not saved between sessions.
- Undo/Redo, Cut/Copy/Paste not implemented.

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
- No real meshes/lighting yet: primitives are wireframes. (step 5)
- Move/rotate/scale handles are not drawn or draggable. (step 4)
- No rectangle (marquee) selection; picking uses icon/centre distance, not mesh raycasts.
- No Persp↔Iso blend animation (it switches instantly); the camera does not slide when the projection changes.
- Scene-view search, draw modes (wireframe), lighting/audio toggles have no effect yet.
- No "Move to View" / "Align With View".
- Icons do not scale with distance (Unity's 3D icons option).
