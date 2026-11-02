# Unity Editor Behaviour Notes

> **Sources:**
> - Unity **6.3 LTS (6000.3.27f1)** is installed at `E:\Unity Related\Unity Versions\`. I opened it on a throwaway
>   project and studied it from **screenshots**. Clicking was impossible: Windows blocked simulated mouse/keyboard
>   input (UIPI, most likely the anti-cheat of a running game), so the menu contents below come from the
>   official Unity Manual (docs.unity3d.com, "Unity's interface" pages).
> - Section 0 lists what was seen live; everything else is manual-based unless marked *(seen)*.
> - We copy *behaviour only*: no Unity code, assets, name, or logo.

## 0. Live observations (Unity 6.3, screenshots)

- **Main toolbar** is near-black (≈#191919). Left: product menu, Account ▾, Asset Store ▾, AI ▾.
  Centre: Play / Pause / Step as a joined button group. Right: cloud, undo-history, search, **Layout ▾**, help, ⋮.
- **Transform tools** live in a **vertical overlay** at the left edge of the Scene view
  (View, Move, Rotate, Scale, Rect, Transform, custom tools). Move is selected by default (blue background).
- **Scene view top overlay:** `Center ▾` (handle position) and `Local ▾` (handle rotation) dropdowns, grid-snap value
  (0.0625), grid/snap buttons, then a group of view toggles; right side: visibility, draw mode, 2D, lighting, audio,
  effects, hidden objects, camera ▾, gizmos ▾.
- **Scene gizmo** top-right: red X, green Y, blue Z cones around a white cube; caption **"< Persp"** under it, plus a lock.
- Scene view shows the **camera** and **light** as billboard icons in the 3D scene.
- **Hierarchy:** `+ ▾`, search with "All" hint, lock and ⋮ in the tab header. Scene root row "Untitled" has a Unity
  logo icon and a ⋮ at the far right; children (Main Camera, Directional Light) have cube icons.
- **Project:** Favorites (All Materials, All Models, All Prefabs), Assets, Packages in the left tree; empty folder
  shows "This folder is empty" centred; icon-size slider at the bottom right.
- **Status bar** shows the newest warning in yellow with a ⚠ icon; right side has small status icons.
- Tabs: dark tab strip, the active tab is lighter with white text, inactive tabs are grey text; every tab header has
  ⋮ and a lock on the right of the dock area.

Each entry: **Name** — what it does — what changes on screen.

---

## 1. Default layout

| Area | Position | Notes |
|---|---|---|
| Menu bar | Very top | File, Edit, Assets, GameObject, Component, Window, Help |
| Main toolbar | Under menu bar | Account/Cloud on the left; Play/Pause/Step centered; Layers + Layout dropdowns on the right |
| Hierarchy | Left column | Tree of GameObjects in the open scene(s) |
| Scene / Game | Center, tabbed together | Scene tab is active by default |
| Inspector | Right column, full height | Properties of the current selection |
| Project / Console | Bottom, tabbed together, under Hierarchy + Scene | Project tab is active by default |
| Status bar | Very bottom | Last Console message on the left; progress/spinner on the right |

- Every panel is a **dockable tab**: drag its tab to dock it elsewhere, to split an area, or out into a floating window.
- Right-clicking a tab (or the ⋮ "more" button) shows: Maximize, Close Tab, Add Tab ▸ (list of windows).
- Double-clicking a tab (or Shift+Space over it) maximizes it; doing it again restores it.
- **Window ▸ Layouts ▸ Default** (also the Layout dropdown on the toolbar) restores the layout above.

## 2. Menu bar

### File
- **New Scene** (Ctrl+N) — opens the New Scene dialog — the Hierarchy is replaced by a fresh scene.
- **Open Scene** (Ctrl+O) — file picker for `.unity` files — the Hierarchy shows the loaded scene.
- **Open Recent Scene ▸** — list of recent scenes.
- **Save** (Ctrl+S) — saves the active scene — the `*` disappears from the scene name in the Hierarchy and title bar.
- **Save As…** (Ctrl+Shift+S) — save dialog.
- **New Project… / Open Project…** — goes back to the launcher (Hub).
- **Build Profiles / Build Settings…** (Ctrl+Shift+B) — build window.
- **Build And Run** (Ctrl+B).
- **Exit** — closes the editor (asks to save unsaved scenes).

### Edit
- **Undo** (Ctrl+Z) / **Redo** (Ctrl+Y) — items show the action name, e.g. "Undo Move".
- **Select All** (Ctrl+A), **Deselect All** (Shift+D), **Select Children**, **Invert Selection** (Ctrl+I).
- **Cut / Copy / Paste** (Ctrl+X/C/V), **Duplicate** (Ctrl+D), **Rename** (F2), **Delete** (Del).
- **Frame Selected** (F) — Scene camera moves to fit the selection.
- **Lock View to Selected** (Shift+F).
- **Find** (Ctrl+F) — focuses the search field of the active panel.
- **Play** (Ctrl+P), **Pause** (Ctrl+Shift+P), **Step** (Ctrl+Alt+P).
- **Project Settings…**, **Preferences…**, **Shortcuts…**.

### Assets
- **Create ▸** Folder, C# Script, Scene, Material, Shader, Prefab… — new asset appears in the Project window in rename mode.
- **Show in Explorer**, **Open**, **Delete**, **Rename**, **Copy Path**.
- **Import New Asset…**, **Import Package ▸**, **Export Package…**, **Refresh** (Ctrl+R), **Reimport**, **Reimport All**.

### GameObject
- **Create Empty** (Ctrl+Shift+N) — adds "GameObject" with only a Transform; it is selected and shown in the Inspector.
- **Create Empty Child** (Alt+Shift+N), **Create Empty Parent** (Ctrl+Shift+G).
- **3D Object ▸** Cube, Sphere, Capsule, Cylinder, Plane, Quad, Text…
- **Effects ▸**, **Light ▸** (Directional, Point, Spot, Area), **Audio ▸**, **Video ▸**, **UI ▸**, **Camera**.
- **Center On Children**, **Make Parent**, **Clear Parent**, **Set as first/last sibling**.
- **Move To View** (Ctrl+Alt+F), **Align With View** (Ctrl+Shift+F), **Align View to Selected**.
- **Toggle Active State** (Alt+Shift+A).

### Component
- **Add…** (Ctrl+Shift+A) — opens the Add Component search popup on the selected object.
- Categories: Mesh, Effects, Physics, Physics 2D, Navigation, Audio, Video, Rendering, Layout, Miscellaneous, Scripts, UI.

### Window
- **Panels ▸ 1…9** (Ctrl+1…9 in older versions: 1 Scene, 2 Game, 3 Inspector, 4 Hierarchy, 5 Project, 6 Animation, 7 Profiler, 9 Audio Mixer).
- **General ▸** Scene, Game, Inspector, Hierarchy, Project, Console (Ctrl+Shift+C) — focuses the window if open, otherwise opens it.
- **Layouts ▸** Default, 2 by 3, 4 Split, Tall, Wide, Save Layout…, Reset All Layouts.
- **Rendering ▸**, **Animation ▸**, **Analysis ▸** (Profiler), **Package Manager**, **Asset Store**.

### Help
- **About**, **Manual**, **Scripting Reference**, **Release Notes**, **Report a Bug…**.

## 3. Main toolbar

- **Transform tools** (left; since Unity 2023 they moved into the Scene view overlay, but older layouts keep them in the toolbar):
  - **View / Hand (Q)** — drag pans the Scene camera; cursor becomes a hand.
  - **Move (W)** — shows the 3-axis arrow gizmo on the selection.
  - **Rotate (E)** — shows rotation rings.
  - **Scale (R)** — shows axis handles with cubes.
  - **Rect (T)** — 2D rectangle handles.
  - **Transform (Y)** — move+rotate+scale combined.
  - The active tool button is highlighted (pressed look).
- **Pivot / Center** toggle — gizmo at the object pivot vs at the selection center.
- **Local / Global** toggle — gizmo axes in object space vs world space.
- **Play** ▶ — enters Play mode: button turns blue, the Game view takes focus, the editor gets a tint.
  Pressing again exits Play mode and resets all changes made during play.
- **Pause** ⏸ — pauses/resumes; can be toggled before Play to start paused.
- **Step** ⏭ — advances one frame while paused.
- **Layers** dropdown — show/hide and lock layers in the Scene view (Default, TransparentFX, Ignore Raycast, Water, UI).
- **Layout** dropdown — switch saved layouts.

## 4. Hierarchy

- Root row shows the **scene name** with a Unity-style scene icon; a trailing `*` means unsaved changes. It has a ⋮ menu (Save Scene, Unload, Remove…).
- **+** button (top left) — the same Create menu as **GameObject**.
- **Search field** — filters the tree by name; matches are shown as a flat list.
- Click selects (row highlighted blue); Ctrl+click toggles; Shift+click selects a range.
- Arrow keys move the selection; ← / → collapse/expand.
- Drag a row onto another to re-parent; drag between rows to reorder.
- Double-click a row → Scene view frames the object. F2 or a slow second click → rename inline.
- Inactive objects are drawn with greyed-out text.
- Right-click → Copy, Paste, Rename, Duplicate, Delete, Create Empty, 3D Object ▸ …
- A new scene contains **Main Camera** and **Directional Light**.

## 5. Scene view

- **Toolbar / overlays:** draw mode (Shaded, Wireframe, Shaded Wireframe…), **2D** toggle, lighting toggle,
  audio toggle, effects dropdown, hidden-objects toggle, **camera settings** (FOV, speed), **Gizmos** dropdown, search field.
- **Scene gizmo** (top-right): XYZ cone axis indicator; clicking an axis snaps the view to it; clicking the centre cube toggles
  Perspective ↔ Isometric. The label under it shows "Persp" or "Iso".
- **Navigation:**
  - Right mouse drag — look around (fly-through); while held, **WASD** move, **Q/E** down/up, **Shift** speeds up.
  - Middle mouse drag — pan. Alt + left drag — orbit around the pivot. Alt + right drag / scroll wheel — zoom.
  - **F** — frame selection.
- The ground grid fades with distance; an infinite XZ grid in 3D mode.
- Left click on an object selects it; drag on empty space draws a selection rectangle.

## 6. Game view

- **Toolbar:** "Game" / "Simulator" switch, **Display 1** dropdown, aspect dropdown (**Free Aspect**, 16:9, 16:10…),
  **Scale** slider (1x…), **Play Focused / Play Maximized / Play Unfocused** dropdown, **Mute Audio**, **Stats**, **Gizmos**.
- With no camera rendering it shows "Display 1 No cameras rendering" in the centre on a black background.
- Shows the output of the active camera(s); during Play mode it receives input.

## 7. Inspector

- Header for a GameObject: active checkbox, name field, **Static** checkbox/dropdown; second row: **Tag** and **Layer** dropdowns.
- Each component is a **foldout** with an enable checkbox (if it can be disabled), its icon, its name, and a ⋮ menu
  (Reset, Remove Component, Move Up/Down, Copy/Paste Component…).
- **Transform** shows Position, Rotation (Euler degrees), Scale as X/Y/Z fields. Dragging the **X/Y/Z label** scrubs the value.
- At the bottom: **Add Component** button → searchable popup.
- Lock icon (top right) keeps the Inspector on the current object even if the selection changes.
- With nothing selected the Inspector is empty.

## 8. Project window

- Two-column layout: **left** folder tree (Favorites, Assets, Packages), **right** contents of the selected folder.
- Top bar: **+** create menu, search field, filter by type, filter by label, save search, hidden-packages toggle.
- Breadcrumb path (Assets > Scenes) above the contents.
- Bottom right: **icon size slider** — far left switches to a list view.
- Double-click a folder → opens it; double-click a scene → opens the scene; double-click a script → opens in IDE.
- Bottom bar shows the path of the selected asset.

## 9. Console

- **Toolbar:** **Clear** (with a dropdown: Clear on Play, Clear on Build, Clear on Recompile), **Collapse**,
  **Error Pause**, **Editor** (player connection) dropdown, search field, and three toggle counters:
  info (ⓘ N), warnings (⚠ N), errors (⛔ N). Clicking a counter hides/shows that message type.
- Each row: icon, timestamp `[12:03:41]`, message first line. Collapse merges identical messages and shows a count badge.
- Selecting a row shows the full message and stack trace in the detail pane at the bottom.
- Double-click opens the source location in the IDE.
- The **status bar** at the bottom of the editor mirrors the most recent message; clicking it opens the Console.

## 10. Play mode behaviour

- Entering Play mode: toolbar Play button is highlighted, Game view is focused, and (by default) the whole editor
  gets a slight tint as a reminder that changes will be lost.
- Exiting Play mode reverts every scene change made while playing.
- Pause keeps Play mode but stops updates; Step advances one frame.
