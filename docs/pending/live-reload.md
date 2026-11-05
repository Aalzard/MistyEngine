# Live reload (branch `live-reload`)

The open editor follows what changes on disk while the project is worked on (by the user in
other tools, or by agents that regenerate scene files and rebuild the editor), instead of the
user closing and reopening it. Code: `crates/misty_editor/src/live_reload/`.

## What it does

1. **Open scenes reload when their file changes** (`live_reload/scenes.rs`). Every loaded scene
   with a file is checked every 0.5 s (`stat`, cheap). A changed file must keep the same time and
   size for 0.3 s before it is read (writers that save in steps are done by then).
   - No unsaved changes in that scene → reloads in place (`Scene::reload_scene`: same scene id,
     path, active state and place in the open set; new entities). Kept: Scene view camera (never
     touched), selection, active object, Inspector targets and expanded Hierarchy rows (matched
     by place: scene file + name and index among same-named siblings, `live_reload/paths.rs`),
     folded scene headers. Play mode is never entered; nothing reloads during Play (picked up
     after Stop). Undo history is cleared, as when opening a scene.
   - Unsaved changes → non-modal bar under the toolbar: "⚠ CyclesRoomDemo.scene changed on disk
     — you have unsaved changes in it. [Reload] [Keep mine]". Keep mine: the next save
     overwrites the file; the bar comes back only if the file changes again.
   - Additive scenes: each scene is handled on its own; the others keep their objects.
   - **The editor's own saves never trigger a reload**: the editor remembers a hash of each scene
     file as it last read or wrote it (`note_disk`, called from `scene_files::load/write` and
     `open_scenes::open_additive`). A changed stamp with the same contents (own save, `touch`,
     checkout of the same commit) is no change. *Why hashes, not times*: file-time resolution
     and clocks differ; comparing contents is exact.
2. **Asset hot reload** (`live_reload/assets.rs` + engine). Audit per type:
   | Type | Before | Now |
   |---|---|---|
   | Textures, models (+ import settings) | renderer polled | same; now reports paths for the toast |
   | Shaders, Shader Graphs | `shader_asset::poll_changes` | same; counted |
   | Effect graphs (.effect) | `vfx::library::poll_changes` | same; counted |
   | Scripts (.lua) | `poll_scripts` | unchanged (no toast: they reload on Play) |
   | Materials (.mat) | only on Ctrl+R | re-read when changed (`AssetDatabase::reload_material`) |
   | Prefabs | cache re-read on use; instances never refreshed | instances refresh (below) |
   | Terrain data (heights, paint, **grass/detail settings**, cliffs…) | read once | `terrain::store::poll_changes` |
   | Timelines, animation clips, Animator Controllers, Audio Mixers | read once | `reload_from_disk` on each store |
   Status bar toast "⟳ Reloaded 3 assets" (4 s) and a Console line naming them.
   - Own writes: editor stores rewrite files with what they hold, so the re-read compares equal
     (`PartialEq`) → no reload, no toast. Clips with unsaved edits (`dirty`) are skipped.
   - Prefab instances: instances that had no overrides against the previous version of the
     prefab become copies of the new one (name, place, position and rotation kept, one undo step
     "Refresh Prefab Instances", scene becomes dirty because instances are stored in the scene);
     instances with overrides are left alone with a Console note. *Why*: there is no per-property
     override merge yet; overwriting overrides would lose work.
   - Terrain: the store remembers the file time and the version at the last read/save; it
     reloads only terrains without unsaved edits (brush strokes save at the end of each stroke).
3. **File ▸ Reload Scene (Ctrl+Shift+R)** — free shortcut (Ctrl+R stays Refresh; Ctrl+Shift+R is
   bound in the specific-modifiers-first table so Ctrl+R doesn't eat it). Asks Save / Don't Save
   / Cancel if a scene has changes (`AfterPrompt::ReloadScenes`), reloads every loaded scene with
   a file, refreshes the asset database and checks assets right away.
4. **Recompile & Restart (dev builds)** (`live_reload/dev_build.rs`, `restore.rs`).
   - Dev build = the compile-time `CARGO_MANIFEST_DIR`'s repo (`../..`) still has `Cargo.toml`
     and `crates/misty_editor/Cargo.toml` (`MISTY_NO_DEV_RELOAD=1` turns it off). Release
     installs don't have it → nothing watched, no menu.
   - Watches `<repo>/target/<profile>/misty.exe` (profile of the running build only: a release
     editor doesn't jump to a slow debug build). Newer = later mtime than the running exe, or same
     mtime and different size; must be unchanged for 1.5 s (linker done). *Why not an embedded
     build id*: build.rs only reruns when it changes, so an id would be stale; copies made with
     Explorer/`std::fs::copy` keep the mtime, copies made with `cp` get a later one (then only a
     real rebuild counts as newer) — both behave.
   - Green "⟳ New build — Restart" pill on the toolbar's right (next to the updater's pill).
   - Help ▸ Developer ▸ **Rebuild & Restart** (egui menus show `&` literally, so a single `&`)
     runs `cargo build -p misty_editor -p misty_player [--release]` in the repo on a thread (no
     console window), progress line + spinner in the status bar, compiler errors as Console
     errors; success → restart flow. Also "Restart Into New Build".
   - Restart: code-editor scripts must be saved; Play stops; unsaved scenes → the usual save
     prompt (`AfterPrompt::Restart`); then the restore file `<project>/Library/RestoreSession.json`
     (project, open scenes + loaded flags + active, camera pose incl. ortho/2D, selection + active
     object, expanded rows, folded scenes, dock JSON + layout name, Code Editor scripts, Project
     folder, Inspector asset), layout + scene view saved, new exe spawned with
     `--restore <file>`, window closes. On startup `misty --restore <file>` opens the file's
     project, applies it after layouts load, and deletes it.
   - **Which exe is started (decision)**: if the running exe is inside `<repo>/target`, the new
     build there. If it is a copy elsewhere (e.g. `E:\MistyTest\grass\misty-grass.exe`), the new
     build is **copied over the copy with the updater's rename trick** (running copy →
     `<name>.old.exe`, or `<name>.<pid>.old.exe` if an older one is still locked; deleted by the
     updater's startup cleanup) and the copy is started; `misty_player.exe` next to it is
     refreshed if present. *Why*: the user keeps their shortcut/location, and `target` never
     holds a running exe, so the next cargo build can replace it (Windows can't overwrite a
     running exe — LNK1104). If the copy can't be replaced (read-only folder), the target build
     is started instead and the Console says so.
   - Rebuild while running the target exe itself: the running exe is renamed aside first (same
     trick) so cargo can write the new one; moved back if the build fails.

## Screenshots (`docs/screenshots/live-reload/`)
- `editor-live-reload.png` (+ `reload-bar-closeup.png`, `new-build-pill.png`,
  `reloaded-toast.png`): CyclesRoomDemo with the bar, the pill and the toast
  (`MISTY_RELOAD_BAR=1 MISTY_RELOAD_TOAST=3 MISTY_FAKE_NEW_BUILD=1`).
- `scene-reloaded.png`: real run on a fresh Standard project; a script rewrote `Main.scene`
  (added "Agent Cube") while the editor ran; it reloaded in place, Cube still selected.
- `restored-session.png`: real `misty --restore <file>` start: selection and camera restored.

## Tests (`live_reload/tests.rs`, `multi_scene.rs`)
Clean reload keeps camera/selection/expanded (kittest); dirty → bar, Reload button (kittest);
Keep mine then save overwrites (kittest); own saves and touches don't reload; additive scene
alone; Reload Scene prompt; prefab instances refresh / overrides kept; materials + timelines
reload, own writes don't; terrain reload / unsaved edits win / own save; restore round trip
(scenes, active, camera, same-named siblings, expanded, folded, dock); pending restore taken
once; newer-build detection + copy-over + target case; settle; `Scene::reload_scene`.

## Left / not done
- The pill click → real process restart wasn't clicked live (unit tests cover exe choice and
  copy-over; `--restore` start was run for real).
- HDRI / lightmap / probe / nav / occlusion stores have their own mtime checks or are rebaked;
  not audited further. Sprites/tile assets: not audited.
- Scenes open in Play mode reload after Stop (by design).
- Prefab overrides aren't merged per property (instances with overrides keep the old version).
- `cargo fmt --all` also reformats files on `main` that others are editing (grass/TAA files);
  those changes were reverted, not committed.
