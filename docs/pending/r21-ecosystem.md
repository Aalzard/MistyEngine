# Round 21E: version control, API reference, accessibility

Branch `r21-ecosystem`. Asked for: git integration in the editor (badges, a Version Control
window, scene diff and merge helper), a generated Lua API reference, and accessibility (screen
readers, editor UI scale, runtime text scale and colour-blind filters). Online relay and NAT
punch-through were out of scope.

## Version control (`crates/misty_editor/src/vcs/`)

Everything goes through the `git` command line (`vcs/git.rs`); there is no libgit2. Each call is
`git -C <folder> -c core.quotepath=off -c color.ui=never …`, with no console window on Windows,
`GIT_TERMINAL_PROMPT=0` and `GIT_OPTIONAL_LOCKS=0` (status never takes the index lock).

- **Discovery**: `git rev-parse --show-toplevel --show-prefix` in the project folder. The project
  may sit inside a bigger repository (like `SampleProject/` in this one): the prefix maps git's
  paths to project-relative ones and status is limited to the project folder (`-- .`).
- **Status** (`vcs/status.rs`): `git status --porcelain=v1 -z --branch --untracked-files=all`,
  parsed into entries (index / work-tree letters, renames) and badges: Modified (M, orange), Added
  (A, green), Untracked (?, grey), Conflicted (!, red), Deleted (D). Folders show the most urgent
  badge inside them. The branch line gives the branch, upstream and ahead / behind counts.
- **Polling** (`vcs/mod.rs`): the editor app starts it; status is read on a worker thread every
  3 s (the editor wakes up for it when idle) and right after any action in the window. Without a
  repository the window says so and shows `git init` to run.
- **Badges**: Project window grid tiles (bottom-left corner of the icon) and list rows (right
  end), with a "Version control: Modified" tooltip; the Hierarchy's scene header shows the scene
  file's badge left of its ⋮ menu.
- **Window ▸ Version Control…** (`vcs/window.rs`): Merge Conflicts / Staged Changes / Changes
  lists with a checkbox per file to stage or unstage, Stage All, a commit message and Commit
  (enabled when something is staged, the message isn't empty and no conflicts remain), Discard
  Changes… (row menu or the diff header) behind a confirmation (tracked files go back to HEAD;
  new files go to the system trash). The right side shows the selected file's unified diff
  (coloured, capped at 5000 lines; binary assets just say so). There are no push or pull buttons:
  the footer shows `git -C "<repo>" push` / `pull` with Copy buttons (and the `push -u` hint when
  the branch has no upstream).
- **Scene diff** (`vcs/scene_doc.rs`, `vcs/scene_diff.rs`): scene files store no object ids, so
  objects are keyed by their Hierarchy path (`Player/Camera`, duplicate sibling names numbered
  `Cube [2]`). Object references (Button targets, joints, LOD renderers… the engine's list, now
  public as `scene::file::object_reference_fields`) are turned from list indices into those paths
  before comparing, so inserting an object doesn't make every reference after it look changed.
  The report lists added / removed / changed objects, components added or removed, and every
  changed field (`Transform.position: (0, 1, 0) → (0, 2.5, 0)`, nested fields as
  `lods[0].screen_height`, references as `→ Player`), plus lighting / navigation changes and a
  "Hierarchy order changed" line. In the window, scenes get a Text / Objects switch.
- **Merge helper** (`vcs/scene_merge.rs`): for a conflicted `.scene`, base / ours / theirs come
  from `git show :1: / :2: / :3:`. Each object (and the scene settings as one row) is Same, Ours
  changed, Theirs changed or Conflict; one-sided changes start on that side, conflicts on ours.
  The Merge window lists what each side did ("Camera.fov: 60 → 70", "added", "deleted"), with
  Ours / Theirs radio buttons and All Ours / All Theirs. Save Merged Scene writes the file
  (parents before children, references re-indexed) and stages it, which marks it resolved.
  Other conflicted files get Use Ours / Use Theirs (`git checkout --ours|--theirs` + add).

Limits: objects are matched by path, so a renamed or re-parented object shows as removed + added,
and deleting the first of two same-named siblings shifts the `[2]` numbering. Prefabs are diffed
as text only.

### Tests never touch a real repository

`vcs::git::git` refuses (in test builds) any folder outside the system temp folder, and sets
`GIT_CEILING_DIRECTORIES` to the temp folder so a temp project can't find a repository above it.
Polling only starts from the editor app, never from `EditorState::new`. Tests make their own
repositories (`git::tests::temp_repo`) and cover: discovery and prefix, status parsing (branch,
renames, folders, conflicts), stage / unstage / commit / discard of modified, untracked and
staged-new files, the window (select, diff, stage by checkbox, commit, discard confirmation, no
push/pull buttons), the scene object diff in the window, scene diff / merge logic, and a real
`git merge` conflict on a scene resolved per object through the merge window.

## API reference (`crates/misty_editor/src/api_docs/`)

- `misty --api-docs <folder>` writes `lua-api.md` and `index.html` (one static page: section
  list, a filter box, light / dark colours). **Help ▸ Scripting Reference** writes them to the
  project's `Library/ApiDocs` and opens the page.
- Sources: every `crates/misty_engine/src/scripting/*.rs` file (embedded with `include_str!`, so a
  released editor can generate it too; a test fails when a new binding file isn't listed) and the
  Code Editor's autocomplete table (`panels/code_editor/api.rs`).
- `api_docs/scan.rs` reads the bindings: `impl UserData for LType` blocks (fields, setters,
  methods, functions; types installed as globals like `Screen` keep the global's name), global
  tables (`let t = g.get("Input")` / `g.set("Mathf", t)` per function, then `t.set("Name", …)`),
  and global functions. The closure's parameter list gives a signature for bindings the
  autocomplete table doesn't know (`|lua, t, i: usize|` → `GetChild(i)`, `Option<…>` → `[name]`,
  `Variadic` → `...`), and the `//` / `///` comment right above a registration becomes the
  entry's notes.
- Each entry has its signature (`Input.GetKey(name)`, `transform:Rotate(eulerDegrees)`,
  `transform.position`), kind (function / property / read-write property), the autocomplete
  one-liner, the source comment and the source file (`api.rs`; no line numbers, so edits elsewhere in
  a binding file do not make the reference stale).
- `docs/api/` holds the generated copy; `api_docs::tests::committed_api_docs_are_up_to_date`
  fails when it is stale (regenerate with `cargo run --release -p misty_editor -- --api-docs
  docs/api`).

## Accessibility

### Editor
- **Screen readers**: eframe's `accesskit` feature (it was already on through eframe's defaults;
  the workspace manifest now names it so it stays on). egui builds the AccessKit tree from widget
  labels. New code names its controls: the Version Control checkboxes are "Stage <path>" /
  "Unstage <path>", the merge radios "Ours for <object>", the commit message field is labelled by
  its caption, Player settings rows label their controls (`labelled_by`), and
  `a11y::name` names icon-only buttons. `a11y::unnamed_controls` lists controls a screen reader
  would announce without a name; tests assert it is empty for the Version Control window and the
  Project Settings ▸ Player page.
- **UI scale**: Edit ▸ Preferences ▸ General ▸ UI Scale (75 % to 200 %), egui's zoom factor, saved
  per user in `hub.json` (`editor_ui_scale`). `MISTY_UI_SCALE=1.5` overrides it for screenshots.

### Runtime (`crates/misty_engine/src/accessibility.rs`)
- Project Settings ▸ Player ▸ **Accessibility**: UI Text Scale (0.5–3×), Colour-blind Filter (Off,
  Protanopia, Deuteranopia, Tritanopia), Filter Strength and Simulate. Stored in
  `PlayerSettings::accessibility`; old projects load the defaults.
- Process-wide settings like `quality`: the player installs them at startup, the editor when the
  project settings load or change (so the Game view previews them) and again when Play stops
  (scripts' changes are dropped).
- **Text scale**: `UiText::scaled_font_size()` is used by drawing, rich text (auto-size limits too),
  input-field carets and auto layout, so layout groups and content size fitters grow with it.
- **Colour-blind filter**: one 3×3 matrix on linear RGB over the final image, after the UI, so menus
  and HUDs are filtered too (`render/color_filter.rs`: copy the output, one full-screen pass back).
  Correction is daltonisation, `I + E·(I − S)` with Machado et al. (2009) severity-1 simulation
  matrices `S` and the usual error-shift `E`; Simulate uses `S` alone. Strength blends with the
  identity. Off (the default) does no GPU work at all, so golden images are unchanged. Applied in
  the player (after the canvases) and the editor's Game view.
- **Lua** (`scripting/accessibility_api.rs`): `Accessibility.textScale`, `.colorBlindMode`,
  `.filterStrength`, `.simulate` (read / write), `.colorBlindModes`, `.minTextScale`,
  `.maxTextScale`; in the autocomplete table and the API reference.
- Tests: matrices (grey stays grey, correction separates red and green for a deuteranope, strength
  blends), settings sanitising and old files, text scale doubling a Text's preferred width, the
  Lua table, a GPU test comparing the filter pass with the CPU matrix pixel by pixel, and the
  Player page editing the settings.

## Screenshots (`docs/screenshots/r21/`)

Version control shots use a throwaway Standard-template project in a temp git repo (changed
scene, new script, staged readme; then a real merge conflict on the scene). Accessibility shots
use the SampleProject (MaterialsDemo, UIShowcase) with the screenshot env options.

- `vcs_window_scene_object_diff.png`: Version Control window, scene Objects view; Project window
  badges (M on Scenes, ? on Scripts, A on Readme.txt).
- `vcs_window_text_diff.png`: the same scene as a coloured text diff, push / pull commands below.
- `vcs_scene_merge_helper.png`: a conflicted scene: Merge Conflicts list with Merge Scene… / Use
  Ours / Use Theirs, and the merge window (two conflicts, one theirs-only change).
- `editor_ui_scale_150_with_scene_badge.png`: Preferences UI scale 150 %; the Hierarchy scene row
  shows the scene file's M badge.
- `color_filter_off.png`, `color_filter_protanopia_simulated.png`,
  `color_filter_protanopia_corrected.png`: MaterialsDemo Game view.
- `color_filter_deuteranopia_simulated_ui.png`, `color_filter_deuteranopia_corrected_ui.png`:
  UIShowcase (the filter covers the HUD and menu canvases too).
- `ui_text_scale_100.png`, `ui_text_scale_150.png`: UIShowcase with the runtime text scale.
- `player_settings_accessibility.png`: Project Settings ▸ Player ▸ Accessibility.

New screenshot env options: `MISTY_VCS=1`, `MISTY_VCS_SELECT=<path>[:objects][:staged]`,
`MISTY_VCS_MERGE=<scene>`, `MISTY_COLOR_FILTER=<mode>[:simulate][:<strength>]`,
`MISTY_TEXT_SCALE=<k>`, `MISTY_UI_SCALE=<k>`, `MISTY_PROJECT_SETTINGS=<Page>[:<Section>]`.

## Not done
- Badges in the Project window's folder tree (only the folder contents show them).
- Partial staging (hunks), history / blame views, branch switching.
- The scene merge helper works per object, not per field; prefabs have no object diff.
- Older editor panels still have controls without accessible names (bare checkboxes and drag
  values in Inspector rows); the audit helper is there to extend the checks panel by panel.
