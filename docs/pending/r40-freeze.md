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
4. `write_atomic` no longer calls `sync_all` (temp file + rename stays).

## Test

`edit_stall_tests`: a project of 3000 files with a terrain, a plane and a cube; the main-thread
time of a selection plus a move must stay under 25 ms (release). Also: the collider material
list comes from the GUID database.

5. The file watch keeps its paths while the editor draws no frames (they were all forgotten
   after 30 s, so the first frame after a quiet spell called `stat` for each on the main thread).

## Measured after the fix (build of this branch, same runs)

- SampleScene on C:, Plane selected: `Panel: Properties` worst frame 108.8 ms -> 0.3 ms; no
  frame over 100 ms.
- SampleScene on a copy of the project on D: while builds ran, three rounds of 75 s without a
  selection, then select the Plane and move it (through the MCP bridge): the select call took
  4 ms / 6.3 s / 38 s before and 3 / 7 / 2 ms after; the `Panel: Scene Tree` frames of 0.7, 25,
  5, 1 and 55.7 s are gone.
- Still seen in that D: run after the fix: `Panel: Properties` 25 s, 9.8 s, 5.8 s and 0.9 s
  (source not found: the profiler has no scope below the panel), `Editor.LiveReload` 32 s,
  `Editor.PollGuids` 2 s and 0.2 s, frames of 3-10 s outside every scope, and shader pipelines
  compiled on the main thread at 1.3-2.5 s each (first use of a material preview; also on C:).
  None of these showed on C:. The project should not live on D: while builds run there.

## Checks

- `cargo fmt --all --check`: clean.
- `cargo test --release -p misty_editor -- edit_stall physics_material prefab thumbnail`:
  29 passed (select + move: 1.7-2.5 ms per round against the 25 ms budget).
- Not run yet: clippy for the workspace, the full test run (the engine's `file_watch` tests
  among them), golden shots (stamp checks under `render/` changed, no drawing code). The new
  timing test was not run against the old code.

## Left

- Find what the Inspector, live reload and the unscoped part of the frame still read from disk
  on the main thread (add profiler scopes per component card and around `save_layout`,
  `vcs.frame`, `mcp::live::frame`, then repeat the D: run).
- A path's first `file_watch::cached_*` call still `stat`s on the calling thread (once per
  path, again after 30 s unused).
- `asset_db::poll` rescans (reads every meta file) on the main thread when files appear.
- Direct `stat`s remain in panels that are rarely open (World Builder preview, search index,
  Code Editor poll every second) and in `trees::cache`, `terrain::harmony::layer_mean`.
- Keep projects off D: while builds run there.
