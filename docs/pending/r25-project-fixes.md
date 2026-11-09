# Round 25: Project window fixes (branch r25-project-fixes)

Bugs reported on v0.3.1, fixed, each with a test. The same problems were looked for in the
Hierarchy, the Scene view and the other windows with inline renaming or context menus.

## Bugs

### 1. Right-click on empty space did nothing
- **Cause**: the folder menu sat on a strip allocated *after* the tiles (`rest`), so it covered
  only the space below the last row: not the gaps between tiles, not the rest of the last row,
  not the breadcrumb area, and nothing in the folder tree. Clicks there hit no widget at all.
- **Fix**: the content area registers one click target over its whole rect *before* the tiles
  and rows (which sit on top and keep their own menus); right-click there opens the folder menu
  (Create ▸ …, Show in Explorer, Refresh), a click deselects. Grid and list view, empty folders
  included. The folder tree got the same background (the open folder's menu), and its Assets
  row, which had no menu, now has the folder menu. Create ▸ in a tree folder's menu makes the
  asset in that folder (it opens first) instead of in whatever folder was open.
- Test: `right_click_on_empty_space_opens_the_folder_menu_everywhere`,
  `create_in_a_tree_folder_makes_the_asset_there`.

### 2. A new folder or asset did not start renaming
- **Cause**: `Create` did start the inline rename, but the field kept the keyboard only until
  anything took it away (see 3), the cursor was put at the end of the name rather than the name
  selected, and an item created out of view was not scrolled to.
- **Fix**: the rename field has its own id, keeps the keyboard (see 3), selects the whole name
  when it starts (typing replaces it, like Unity) and scrolls the item into view.
- Test: `a_new_folder_starts_renaming_with_its_name_selected_and_keeps_the_field` (Create ▸
  Folder from the empty-space menu, 2 s of frames, focus, full selection, type, Enter).

### 3. Right-click ▸ Rename lost the field after about half a second
- **Cause**: the field ended the rename on *any* loss of keyboard focus, and its focus was
  fragile: an automatic widget id (it moves when the items before it change, e.g. a refresh
  from the file watcher; egui then drops the focus as "the focused widget vanished"), and
  `ui.put`, which advanced the grid's layout so the tiles after the renamed one jumped under the
  pointer. Whatever took the focus for one frame committed the unchanged name and closed the
  field. The exact trigger in the reported session was not reproduced in the test harness (the
  menu ▸ Rename path kept its field there); the fix removes every way the field could end
  without the user ending it.
- **Fix**: stable id, `ui.place` (no layout change), and a lost focus only ends the rename when
  the user did it: a click elsewhere or a key (Enter, Tab, Escape) in the last 0.3 s
  (`hold_rename::focus_dropped`). Otherwise the field takes the focus back. Selection is kept.
  A folder renamed from the tree shows one field (on its tree row): with the stable id the tree
  row and the tile would otherwise share it and every key would be typed twice.
- Test: `context_menu_rename_keeps_its_field_and_selection` (2 s of frames, a refresh and a
  forced focus drop survive; a click elsewhere still ends it).

### 4. Dragging files or folders from the desktop imported nothing
- **Cause**: nothing read egui's `dropped_files` / `hovered_files`; OS drops were ignored.
- **Fix** (`os_drop.rs`): dropped on the Project window, files and folders are copied
  (recursively, under a free name, a file's `.meta` along when the name is unchanged) into the
  folder tile or tree row under the pointer, else the open folder; then a refresh runs the
  import (meta files and GUIDs, model materials, script inspection) and the new item is
  selected. While files hover, the target folder and the content area are outlined with
  "Import into Assets/…". On the Hierarchy or Scene view they are imported into the open folder
  and models and prefabs are instantiated. Dropped anywhere else: imported into the open folder
  and the Project tab is brought forward. New action `ImportFiles`.
- Note: on Windows, winit does not move the pointer during an OS drag, so "the folder under the
  pointer" is the last one the pointer was over before the drag entered; the outline shows it.
- Test: `files_dropped_from_the_os_are_copied_imported_and_shown` (folder with a sub-folder onto
  empty space, a file onto a folder tile; checks copies, import and listing) and
  `os_drop::tests::folders_copy_with_everything_in_them_under_free_names`.

### 5. Show in Explorer opened Documents
- **Cause**: `explorer` got `"/select,E:\My Game\…"`: the standard library quotes the whole
  argument once the path has a space, `explorer` does not understand that and opens its default
  folder (Documents). Folders were also selected in their parent rather than opened.
- **Fix** (`explorer.rs`): the command line is written raw: `explorer "<folder>"` opens a
  folder itself, `explorer /select,"<file>"` selects a file; slashes, `\\?\` and trailing
  separators are tidied. macOS: `open` / `open -R`. Linux: `xdg-open <folder>`, files via the
  file manager's D-Bus `ShowItems` (falls back to opening the parent). The Hub's project list
  still selects the project folder in its parent.
- Test: `explorer::tests::*` (Windows, macOS and Linux commands for paths with spaces; a real
  temp folder opens, a file is selected).

### 6. New files appeared only after leaving the folder
- **Cause**: the Project window listed a folder when it was opened and on refreshes. The only
  watcher was the GUID walk (every 2 s, one walk behind), which runs only while frames are
  drawn: an idle editor draws none, so nothing noticed files added by Explorer until the user
  navigated (which lists the folder, without importing).
- **Fix**: `ProjectBrowser::poll_disk` (every 250 ms of frames) compares the modification time
  of every listed folder (the Assets tree and the open folder) from the background file watch;
  a changed one is listed again and, when its names differ, the editor refreshes (re-list,
  folder tree, import). Saving a file in place or through a temp file keeps the names, so it
  costs nothing. `file_watch` gained `pin` (paths watched while nobody asks) and `set_waker`:
  the File Watch thread asks for a repaint when a watched stamp changes, so an idle editor
  wakes up for it. The 0.5 s search cache was not the cause (refreshes drop it).
- Test: `files_added_on_disk_show_up_without_leaving_the_folder` (a file and a folder added,
  then a file removed, outside the editor), `file_watch::tests::pinned_paths_stay_watched_and_changes_wake`.

## New: press and hold to rename
- Hold the primary button 1.5 s on a Project tile, list row or tree folder, or a Hierarchy
  object: its rename starts. Moving more than 5 px (a drag) cancels; modifiers cancel. A thin
  bar along the item's bottom fills from 0.25 s. The release that follows (often beside the
  name field) does not end the rename (`hold_rename::keep_focus`). F2 and the menus still work.
- Tests: `hold_rename::tests::*` (timing, slop), `press_and_hold_renames_a_project_item_after_the_hold_time`
  (not at 1.2 s, yes at 1.7 s, survives the release, a drag cancels),
  `press_and_hold_renames_a_hierarchy_object`.

## Sweep
- **Hierarchy**: right-click on empty space only worked below the rows; same background fix
  (`right_click_on_empty_hierarchy_space_opens_the_create_menu`). Its rename field got the same
  stable id, full selection and focus rules.
- **Rename fields that could never end**: Timeline track names, Input Actions maps / actions /
  composites, Shader Graph and Script Graph group titles asked for the focus every frame and
  then checked `lost_focus()`, which is then always false: Enter and clicking away did nothing
  (Input Actions: only Enter). They now use `hold_rename::field` (stable id, name selected,
  Enter / click away commit, Escape cancels, focus drops survive);
  test `rename_fields_select_the_name_keep_focus_and_end_on_enter_click_or_escape`.
  Animator parameters and Audio Mixer names already used the right pattern.
- **Timeline**: right-click on the empty track-header area now opens the Add menu.
- **Show in Explorer elsewhere**: search palette and Assets menu use the fixed action; the Hub
  uses the select variant.
- **OS drops on Hierarchy / Scene view**: added (import, then instantiate models and prefabs).

## Left
- Build window's "Open BuildReport.txt" runs `explorer <file>` (opens the file in its app; fine
  with spaces, not changed).
- Rows of the Hierarchy are laid out only when visible: a rename started (F2) on a row scrolled
  out of view shows no field until it is scrolled back.
- Dropping onto a specific Hierarchy row (as a child of that object) imports and instantiates at
  the root.

## Screenshots (`docs/screenshots/r25/`)
- `rename-after-create.png`: `MISTY_CREATE=Folder` (new capture option) — the new folder with
  its name selected.
- `drop-highlight.png`: `MISTY_DROP_PREVIEW=1` (new capture option) — the Project window's
  drop outline and caption.

## Checks
- `cargo fmt --all`; `cargo clippy --workspace --all-targets --release`: 0 warnings.
- `cargo test --workspace --release --no-fail-fast` (test binaries run one at a time): editor
  794 passed / 0 failed (79 ignored), editor MCP end-to-end 1 passed, engine 1379 passed / 0
  failed (92 ignored), player 11 passed, dedicated server 1 passed. An earlier full editor run
  had the known-flaky `new_project_page_shows_the_meadow_demo_card` fail once under load.
