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

## The hung test run (2026-11-12)

Cause: not a lock deadlock and not the disk alone. `edit_stall_tests` installed the GUID
database of their own temp project (1500 files, no meta files) with `asset_db::install` and
left it installed for the rest of the test process. `AssetDatabase::refresh` called
`asset_db::rescan()`, which rescans *whatever* database is open: every later test that
refreshed (Project window, Code Editor, World Builder...) scanned the edit-stall project and
wrote a meta file per asset, each through `write_atomic` with `sync_all`. With TEMP on D: at
1-3 s per flushed write, and several tests doing it at once and again on every refresh, 16
tests ran for an hour.

Measured (TMP/TEMP `C:\MistyTry\r40tmp`, branch at 20cb44b2):
- The 16 tests alone: 16 passed in 176 s (several over 60 s, as before this branch: they are
  heavy harness tests run 16 at a time).
- The 16 plus `edit_stall_tests`: 18 passed in 177 s, but other tests had written 360 meta
  files into the edit-stall project (`misty_edit_stall_<pid>/Assets/Bulk/...`) while it ran.
- The same 16 on the branch base (3609d4e2): not measured; that run waited for the shared
  target's build lock and hit its time limit while compiling (the base never installs a
  database in editor tests, so it cannot take this path).

Fix (e895a3b7):
- `asset_db::rescan(root)` and `asset_db::poll(root)` take the caller's project and do
  nothing while the open database is another project's (`AssetDatabase` passes its root).
- Scans run without a lock; their result replaces the database only if the slot still holds
  the same project (`replace_if_same`), so a project installed meanwhile is not overwritten.
  `compare` (the watcher) does the same. No lock is held across a scan, flush or join (the
  watcher's walk is joined only once finished).
- `edit_stall_tests` install their database one at a time (a mutex) and put the previous one
  back when done.
- Regression tests: `asset_db::tests::rescanning_a_project_leaves_another_projects_database_alone`
  (rescan and 20 polls for project B while A's database is open: no meta files in A, a 60 s
  limit on a thread), `a_database_installed_during_a_rescan_is_kept`, and
  `edit_stall_tests::refreshing_a_project_leaves_another_projects_database_alone` (an
  `AssetDatabase` refresh + poll of B with A's 300-file database installed: no meta files in A,
  120 s limit). Without the fix the old `rescan()` scans A: the meta count fails on an SSD
  and the time limit trips on a slow disk.

## Checks (2026-11-12, at e895a3b7 plus these notes)

D: (the old target and TEMP) is failing (Windows: "Full Repair Needed", bad blocks): builds
there read back corrupt `.rmeta` files (rustc ICEs decoding `sha2`, `rapier2d`, `exr`, `egui`
even after a clean). Everything below ran with target `E:\MistyTargets\r40-freeze` (fresh
build), TMP/TEMP `C:\MistyTry\r40tmp`, slots from `/e/MistyTargets/build-slot.sh`, `-j 4`,
logs in `C:\MistyTry\r40logs`.

- `cargo fmt --all --check`: clean.
- `cargo clippy --workspace --all-targets --release`: 0 warnings.
- Targeted: `asset_db saver file_watch` (engine) 26 passed; `edit_stall_tests`, Code Editor,
  Project window, World Builder and `assets::` tests (editor) 31 passed, the editor ones in
  12 s.
- Without the fix (old `rescan()` restored for one run): the new
  `refreshing_a_project_leaves_another_projects_database_alone` fails with 300 meta files
  written into the other project.
- `cargo test --workspace --release --no-fail-fast`: 2374 passed, 1 failed. The editor's 873
  passed in 227 s (no test over 60 s in the Code Editor / Project / World Builder groups that
  hung before). The failure: `render::vgeo::gpu_tests::auto_picks_one_phase_and_lit_for_a_few_rocks`
  (line 353, `stats.visibility` false: the Auto visibility buffer decides by GPU counters read
  back "a frame or two ago", 8 frames). Alone on this branch: failed once, then passed twice;
  on the base 3609d4e2: passed three times. Not touched by this branch (no `vgeo` change); a
  SubCraft build was using the same GPU. Flaky under GPU load.
- Golden shots (`cargo test --release -p misty_engine --lib golden_shots -- --ignored
  --nocapture --test-threads=1` under `MistyWork/gpu-timing.lock`): 20 of 20 ok (largest mean
  0.39, at most 0.03 % changed).

## Next steps

Merge. Optional later: asynchronous pipeline builds; `model_settings` reading the importer
every frame; the vgeo test could render more frames before reading the Auto choice.

Built editor of the earlier state at `C:\MistyTry\r40\misty-r40.exe` (0296451d); run automated
editors with `MISTY_WINDOW_SIZE=1100x650`.

## Left

- Asynchronous pipeline builds (see above).
- `model_settings` reads the model's importer from disk every frame while a model asset is
  inspected.
- A path's first `file_watch::cached_*` call still `stat`s on the calling thread (once per
  path, again after 10 minutes unused).
- `asset_db::poll` rescans (reads every meta file) on the main thread when files appear.
- Direct `stat`s remain in panels that are rarely open (World Builder preview, search index,
  Code Editor poll every second) and in `trees::cache`, `terrain::harmony::layer_mean`.
- Keep projects and build targets off D: (failing disk).
