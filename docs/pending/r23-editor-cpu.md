# Round 23: editor CPU frame (branch r23-editor-cpu)

Asked for: make the editor's main thread fast. Round 22 found the Scene view CPU bound
(15.6 ms editor frames against a 6.0 ms GPU frame on MeadowDemo), so GPU savings did not show
in the editor. Measure first with a repeatable bench, then cut the main-thread frame towards the
GPU frame (at least 30 %), keep behaviour the same, test every cache, commit each win with its
numbers.

## Result

Before = main at `bb63be9f`, after = this branch (`a06d954b`), both release builds, run
alternately (before, after, before, …) three rounds each on a quiet machine, MeadowDemo in the
Scene view, `MISTY_VSYNC=0`, median of the rounds (every round listed in the notes below).

| measure | before | after | change |
|---|---|---|---|
| **Main-thread work in `ui()`**, fixed camera (profiled: pre-frame + panels) | 6.22 ms | **5.01 ms** | −19 % |
| — live reload / script / asset polls (`Editor.GameFrame.Pre`), mean / worst frame | 0.24 / 11.9 ms | **0.06 / 0.9 ms** | spikes gone |
| — `SceneView.Gather` (draws, gizmo lines, scene queries) | 1.21 ms | **0.31 ms** | −74 % |
| — Scene view overlays (icons, handles) | 0.20 ms | **0.06 ms** | −70 % |
| — Project window | 0.37 ms | **0.34 ms** | −8 % (more in big folders and while searching) |
| — `Render.Submit` (wgpu encoding) | 1.85 ms | 2.00 ms | not changed (see below) |
| Frame interval, fixed camera: median | 8.00 ms | 8.07 ms | GPU bound: GPU frame 7.9 ms in both |
| Frame interval, fixed camera: p99 (profiled runs) | 19.9 ms | **11.3 ms** | −43 % |
| Flythrough: frame interval median / p99 | 7.45 / 19.8 ms | 7.37 / **10.6 ms** | GPU bound (6.6–7.0 ms); p99 −46 % |
| Flythrough: editor `ui()` CPU median | 5.93 ms | **4.92 ms** | −17 % |
| **Low quality** (GPU frame 2.3 ms, so CPU bound), fixed camera: frame median | 6.42 ms (156 FPS) | **5.18 ms (193 FPS)** | −19 % |
| Low quality flythrough: frame median / `ui()` CPU (2 rounds, mean) | 6.31 / 5.07 ms | **5.12 / 3.89 ms** | −19 % / −23 % |

The Scene view is GPU bound now on the default quality level, with the camera still or flying:
the frame equals the GPU frame plus present, so the CPU savings show as main-thread headroom
there and as frame time where the GPU has room (Low quality: 156 → 193 FPS). The 30 % cut of the
main-thread frame was not reached: −19 % in `ui()` (−23 % flying on Low); the biggest piece
left, `Render.Submit` (2 ms), is wgpu's command encoding (below).

Round 22's 15.6 ms flythrough was not reproduced by the unchanged build on this machine today
(7.0–7.6 ms, GPU bound): the first runs of this round, on a fresh `Library/`, still showed
14.5–15.6 ms while grass chunks, tree lists and Misty GI distance fields were being built and
cached; once cached, the flythrough is GPU bound with either build.

## The bench

- **Fixed camera** (the main number): `MISTY_SCREENSHOT=out.png MISTY_SCENE=Assets/Scenes/MeadowDemo.scene
  MISTY_TAB=Scene MISTY_CAPTURE_FRAME=900 MISTY_VSYNC=0 misty SampleProject`. The capture run
  prints the median, mean, p90 and (new) p99 of the editor frame interval over frames 450–900.
  The Scene view camera is the scene's default one (capture runs never save the view).
- **Per-stage CPU**: the same run with `MISTY_PROFILE=prof.jsonl`; mean ms per scope over frames
  300–900 (a small script summed the scopes). New scopes: `SceneView.Gather` split into
  ScriptGizmos / Draws / Gizmos / SceneQueries / Settings, `DrawCache.Walk / Meshes / Extras`,
  `Render.Submit.Finish / Queue`.
- **Flythrough**: `MISTY_CAMERA_BENCH=600 MISTY_CAPTURE_FRAME=800` (the round-22 bench): frame
  interval, the editor `ui()` CPU time, GPU frame.
- Every comparison ran the two binaries alternately (A, B, A, B, …) under the GPU-timing lock and
  takes the median of the rounds. Most of the round ran while another project (SubCraft) built
  on the same machine: those rounds were noisy (single runs at 2–9× the quiet times, dropped when
  obvious); the final table was measured after it finished, on a quiet machine.

## What the profile showed (before)

Fixed camera, quiet machine (mean ms per frame): frame 8.3 ms against a 6.7 ms GPU frame.
`Editor.Panels` 6.8 (Scene view 5.9: `Renderer::render` 4.0 of which `Render.Submit` 2.1,
`SceneView.Gather` 1.5, overlays 0.23; Project 0.43, Hierarchy 0.24), `Editor.GameFrame.Pre` 0.40
on average but 17–23 ms every half second (live reload 11–13, script poll 3–5, renderer asset
poll 3.5–5.3: `stat` calls on the main thread), paint and present the rest.

## Changes (one commit each)

1. **Bench and scopes** (`793d5d12`): p99 in capture runs, the sub-scopes above.
2. **File stamps from a background thread** (`21b2a890`). New `misty_engine::file_watch`: with
   `set_background(true)` (the editor's `main`) a "File Watch" thread stats every watched path
   every 200 ms; pollers read its last stamp with a map lookup (`cached_metadata` /
   `cached_modified`, `vfs::cached_modified`). Used by every poller: textures and their import
   settings, models and their meta, shaders and their dependencies, effects, terrains, scripts,
   editor scripts, open scenes, materials and prefabs. **A cached stamp only answers "nothing
   changed"**: when it differs from the recorded one the poller stats the file again directly
   and decides on that fresh stamp, exactly as before (`live_reload::changed_stamp`), so the
   editor's own saves (recorded right after writing) are never taken for outside changes, and
   reloads still wait for files to settle. A change is seen at most one pass (200 ms) later.
   Off (tests, the player, tools) everything stats directly, as before. Paths not asked about
   for 30 s stop being watched. Result: `Editor.GameFrame.Pre` worst frame 23.3 → 0.9 ms, mean
   0.40 → 0.07 ms; frame p99 25.5 → 18.1 ms in that run.
3. **Gizmo lines** (`47f119b7`, committed as WIP when the session was paused; measured with
   the next one): `gizmos::scene_lines` worked out every object's world matrix and active state
   and only then checked the selection; it now skips the walk without a selection and checks the
   selection first (same lines, same order). `SceneView.Gizmos` 0.40 → under 0.03 ms.
4. **Scene view gather** (`60b11d0b`). The per-frame lists walked the whole Hierarchy with a
   parent-chain active check for components the scene doesn't have: reflection probes (twice),
   light probe groups (twice), fog volumes, decals, water, and the object icons (seven component
   lookups per object). `Scene::has_any::<T>()` (a query over the component's archetypes)
   returns the empty list at once, `icon_objects` starts from component queries, and
   `Scene::walk` borrows each children list instead of copying it; the lists are the same, in the
   same order. The draw cache keeps an unchanged renderer's entry instead of copying its mesh and
   renderer every call. `SceneView.Gather` 1.46 → 0.30 ms, overlays 0.23 → 0.05 ms.
5. **Project window** (`dbc96c36`): tiles and list rows scrolled out of view keep their place and
   interaction but paint nothing; the folder tree no longer copies the whole Assets and Packages
   trees every frame; a search's results are reused while its text stays the same, until the
   browser refreshes and for at most 0.5 s (it walked `Assets/` on disk every frame).
   `Panel: Project` 0.43 → 0.31 ms (more in folders with many tiles, and while searching).

## Caches and invalidation (tests)

- `file_watch`: `without_the_thread_every_call_stats_the_file`, `cached_stamps_change_on_the_next_pass`
  (a rewrite, a delete and a re-create each show on the next pass), `unused_paths_stop_being_watched`;
  `live_reload::tests::changed_stamp_reports_only_real_changes`. The existing live-reload tests
  (scenes, materials, prefabs, terrains, conflicts) run with the cache off and still pass.
- Draw cache: `kept_entries_never_hide_a_later_change` (after unchanged calls: another mesh,
  material path, renderer flag, a move, disable / enable; every call must equal a fresh gather).
- Project search: `cached_search_never_shows_a_stale_list` (new text, refresh after a new file,
  a file deleted outside the editor gone within `SEARCH_CACHE`).
- The gather fast paths return exactly what the walks returned (the same functions, early-out
  only when no object has the component); the scene-level tests of those lists still pass.

## Looked at, not kept

- **`Render.Submit` on a worker thread.** wgpu encodes the whole frame in `CommandEncoder::finish`
  (1.6–1.8 ms of the 2.1 ms `Render.Submit`; `Queue::submit` 0.25–0.5). It is CPU work in
  wgpu-core (validation and Vulkan recording of every pass), not GPU or surface waits; the
  editor runs more passes than the headless benches (grid, outline, overlays, gizmo lines, both
  views' targets) and the Profiler adds timestamp queries. A version that hands the encoder to a
  "Render Submit" thread and waits for it after the panels worked (identical images, GPU test)
  but saved nothing: the Scene view is the last dock panel drawn, so only its overlays (0.05 ms)
  ran between the hand-off and the wait, `finish` ran ~30 % slower on the worker
  (2.1 ms), and the wait took 2.2 ms. To pay off, the wait has to move past the rest of the frame
  (egui's tessellation, ~1–2 ms), i.e. into an egui-wgpu paint callback that owns the pending
  submit and the Scene view's canvas commands. Kept as a patch outside the repo.
- **Change ticks on the scene.** `Scene::world` is a public hecs `World` that scripts, physics
  and the editor write directly, so a scene-wide tick could not be made reliable (one missed
  write path = a stale view). The draw cache keeps comparing its inputs; the other lists became
  cheap enough without a cache.
- The GUID watcher's rescan (`asset_db::poll`) still runs on the main thread when a background
  walk finds new or removed files (one 26–150 ms frame when assets appear, e.g. the meta files an
  editor run writes); moving it means making the scan race-free with the editor's own moves.

## Rounds (final comparison)

Fixed camera, unprofiled frame medians: before 7.77 / 8.00 / 8.00, after 7.87 / 8.10 / 8.07 ms.
Profiled: main-thread work 5.87 / 6.22 / 6.22 → 4.91 / 5.07 / 5.01 ms; p99 18.2 / 19.9 / 20.2 →
9.9 / 11.6 / 11.3 ms. Flythrough: frame 7.01 / 7.45 / 7.64 → 7.37 / 7.63 / 7.25 ms, p99 19.8 /
20.0 / 19.7 → 9.3 / 10.6 / 18.3 ms, `ui()` 5.45 / 5.93 / 6.10 → 4.41 / 4.98 / 4.92 ms. Low quality:
6.42 / 6.55 / 5.71 → 5.18 / 5.18 / 5.30 ms; flying 5.90 / 6.71 → 4.78 / 5.46 ms.

## Not done yet

- `Render.Submit` stays 1.6–1.8 ms on the main thread (see above).
- The flythrough is GPU bound now; its remaining CPU is the renderer's own (grass updates, culling,
  asset preparation, 1–3 ms) and egui's paint and present.
- Hierarchy (0.2 ms, already virtualised) and Inspector were cheap in this scene; not measured on
  a scene with thousands of rows.
