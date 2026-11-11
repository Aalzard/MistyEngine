# r40: editor "Not Responding" on selection and edits

Branch `r40-freeze` (from main 3609d4e2). Target `D:/MistyTargets/r40-freeze`.

## What the user saw

"It keeps becoming Not Responding with every small touch": selecting the plane, moving it.
Earlier: freezes every couple of seconds, for half a minute or a minute.

## Where it was

The user's editor (`C:\MistyTry\misty.exe`, started from the Hub) had the project
`D:\MistyTargets\try-main\SampleProject` open: D: is the slow hard disk the agents' cargo builds
write to (disk queue 6, 70-200 ms per read or write while they run). The editor did file-system
calls on the main thread every frame or every couple of seconds. On an SSD they cost micro- or
milliseconds; on that disk each one can wait for seconds.

Measured with the user's exe (copy), `MISTY_PROFILE` captures and the same project copied:

| Main-thread disk access | SSD (C:) | Busy hard disk (D:) |
| --- | --- | --- |
| Collider Inspector: Physics Material list walked `Assets/` (1842 entries) every 2 s | 104-117 ms per walk (`Panel: Properties`) | 71.5 s for one cold walk |
| Scene Tree: a `stat` per prefab row per frame (`AssetDatabase::prefab`) | microseconds | one frame of 25.5 s (`Panel: Scene Tree`) |
| Saves through `asset_db::write_atomic` (`sync_all`, new since v0.3.2): a material is saved on every frame of a slider drag | 2.5 ms median, 69 ms max | 0.9 s median, 27.5 s max |
| Thumbnails: a `stat` per visible image or material per frame | microseconds | same risk as the prefab row |

Not the cause (measured): the weather preview (WeatherDemo with Preview on, Weather selected:
6 ms frames, `Panel: Properties` 0.85 ms, no weather scope above 1 ms), undo snapshots, the
first-open import, memory (28 GB free). The main thread at 100 % of a core is the uncapped frame
loop (`editor_vsync` is off, 165-200 FPS).

## Fixes

1. `physics_material_editor::project_materials` lists from the open GUID database (no disk).
2. `AssetDatabase::prefab`, `Thumbnails::get`, `MaterialPreviews::thumbnail`, the texture
   preview, terrain palette thumbnails and swatches, tile palette textures: stamps from the
   background file watch (`file_watch::cached_modified`); a differing stamp is checked on disk
   where a decision depends on it.
3. Engine polls every half second (terrain layer textures, reflection probes, light probes,
   decal materials): the file watch's stamp first, the disk only on a difference.
4. `write_atomic` keeps `sync_all` (the PC lost power twice on 2026-11-11). Saves made on every
   change go through `asset_db::saver` instead: a writer thread that keeps the latest bytes per
   path and writes a file at most every 0.5 s. `AssetDatabase::update_material` (every frame of
   a slider drag) uses it. Flushed before files are read back (`AssetDatabase::refresh`),
   renamed, moved, duplicated, deleted or built, and on exit; live reload leaves a material
   alone while its save is on the way; failed writes go to the Console. Off outside the editor
   (tests, player: the save is written at once).

## Test

`edit_stall_tests`: a project of 1500 files with a terrain, a plane and a cube; the main-thread
time of a selection plus a move must stay under 25 ms (release). Also: the collider material
list comes from the GUID database.

5. The file watch keeps its paths while the editor draws no frames (they were all forgotten
   after 30 s, so the first frame after a quiet spell called `stat` for each on the main thread).

6. The file watch forgets an unused path after 10 minutes (30 s before): a path asked about
   again costs a `stat` on the asker's thread.
7. Profiler scopes: `Inspector.Header`, `Inspector.PrefabRow`, `Inspector: <component>`,
   `Inspector.Material`, `Inspector.AddComponent`; `LiveReload.Scenes`, `.Assets`, `.Terrains`,
   `.Stat`; `FileWatch.FirstStat`; `Editor.Shortcuts`, `.Bars`, `.AfterPanels` (`.Previews`,
   `.Windows`, `.Vcs`, `.Mcp`, `.Actions`, `.EndFrame`, `.Updater`, `.SceneFiles`,
   `.SaveLayout`).
8. `MISTY_WINDOW_SIZE=1100x650`: a small window at the top left, not maximised, not remembered
   (automated runs).

## Measured after the fix

Runs: SampleScene, three rounds of 75 s without a selection, then select the Plane and move it
through the MCP bridge, with `MISTY_PROFILE`.

- C:, Plane selected (capture run): `Panel: Properties` worst frame 108.8 ms -> 0.3 ms.
- Project copy on D: under build load, main 3609d4e2: the select call took 4 ms / 6.3 s / 38 s;
  `Panel: Scene Tree` frames of 0.7, 1, 5, 25 and 55.7 s.
- Same with the first four fixes: select 3 / 7 / 2 ms, Scene Tree stalls gone, but
  `Panel: Properties` 25 s, 9.8 s, 5.8 s, 0.9 s, `Editor.LiveReload` 32 s, `Editor.PollGuids`
  2 s.
- Same with everything above, two runs (D: queue length 6-20, 1.5-3 s per read during the
  second): select 4 / 2 / 0 ms and 2 / 1 / 2 ms; worst frame 206 ms and 791 ms; no Properties,
  live reload or Scene Tree frame over 100 ms. The 25 s and 32 s frames did not come back, so
  their source was never seen inside one of the new scopes: the likely one is the `stat` for
  paths the file watch had forgotten after 30 s (fix 6).
- A frame of 736 s in a third run was the gap before the frame (every scope of it starts at
  735 956 ms): the window drew nothing, as when it is covered or minimised. Not a stall.
- Shader pipelines still compile on the main thread the first time they are needed (1.3-2.5 s
  each under build load, `Pipeline: mesh`, `Pipeline: custom shader`); the pipeline cache in
  `Library/ShaderCache` makes later sessions fast. All lazy pipeline builds go through
  `render/pipelines.rs`; building them on a thread with a placeholder draw is a renderer-wide
  change, not done here.

## Checks (state at the hand-over, 2026-11-11 23:15)

Nothing of r40 is running and no build slot is held. HEAD builds (release editor built at
0296451d; the commit after it only changes a test constant and these notes).

- `cargo fmt --all --check`: clean.
- `cargo clippy --workspace --all-targets --release`: 0 warnings (at 43346d46, log
  `D:\MistyTargets40-logs\clippy.log`).
- Targeted tests at 0296451d: `cargo test --release -p misty_editor -p misty_engine --
  edit_stall physics_material file_watch saver live_reload material`: 34 + 48 passed
  (`D:\MistyTargets40-logs	1.log`).
- **Full test run: NOT finished, and it needs a look before merging.**
  `cargo test --workspace --release --no-fail-fast` (log `D:\MistyTargets40-logs	est.log`)
  passed 441 of the editor's 956 tests in 7 minutes, then printed nothing for an hour with 16
  tests "running for over 60 seconds" and never finishing; I stopped it. No failure or panic in
  the log. The 16: `panels::code_editor::tests::` clicking_a_console_error_opens_the_line,
  closing_a_modified_file_asks_to_save, double_click_in_project_opens_the_script,
  f5_plays_and_completion_accepts_with_tab, new_scripts_open_and_follow_renames,
  syntax_errors_show_after_a_pause, typing_marks_dirty_and_ctrl_s_saves;
  `panels::project::tests::` create_duplicate_and_delete_from_the_context_menu,
  moving_and_renaming_scenes_updates_build_settings,
  renaming_a_material_updates_scenes_and_the_open_scene; `panels::world_builder::tests::`
  live_in_scene_view_pushes_the_preview_and_reverts, preview_card_shows_modes_and_live_toggle,
  preview_follows_edits_coarse_then_sharp, stamp_inspector_edits_the_component,
  template_picker_applies_a_template_as_one_undo_step,
  terrain_inspector_button_opens_the_world_builder.
  Not known which it is:
  1. A deadlock from this branch. First suspect: `edit_stall_tests` call
     `asset_db::install` (a write lock on the global GUID database) while other tests read it;
     a test thread that takes the read lock twice (`with_project` -> `guid_of`...) deadlocks
     when a writer queues between the two. The background saver and file watch are off in
     tests.
  2. The disk: TEMP was on D: (1.5-3 s per read at the time) and every asset save calls
     `sync_all` again since the revert.
  Next step: run those 16 tests alone, with TEMP on C:, on this branch and on main; if they
  hang only here, give `edit_stall_tests` their own database handling (or drop the `install`
  loop) and rerun.
- Golden shots: not run (`cargo test --release -p misty_engine --lib golden_shots -- --ignored
  --nocapture --test-threads=1` under `MistyWork/gpu-timing.lock`). The `render/` changes are
  file stamp checks in `terrain_layers.rs`, `probes/store.rs`, `decals/mod.rs`.
- The new timing test was not run against the old code.

## Next steps

1. The hung test run above.
2. Golden shots.
3. Full `cargo test --workspace --release --no-fail-fast` to the end.
4. Optional: asynchronous pipeline builds; `model_settings` reading the importer every frame.

Use only `/d/MistyTargets/build-slot.sh` for slots. Target `D:\MistyTargets40-freeze`, a
built editor at `C:\MistyTry40\misty-r40.exe` (0296451d), run automated editors with
`MISTY_WINDOW_SIZE=1100x650`.

## Left

- Asynchronous pipeline builds (see above).
- `model_settings` reads the model's importer from disk every frame while a model asset is
  inspected.
- A path's first `file_watch::cached_*` call still `stat`s on the calling thread (once per
  path, again after 10 minutes unused).
- `asset_db::poll` rescans (reads every meta file) on the main thread when files appear.
- Direct `stat`s remain in panels that are rarely open (World Builder preview, search index,
  Code Editor poll every second) and in `trees::cache`, `terrain::harmony::layer_mean`.
- Keep projects off D: while builds run there.
