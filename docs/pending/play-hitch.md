# Freeze on entering Play / opening MeadowDemo (branch play-hitch)

User report: the release editor's capture run of MeadowDemo in Play mode shows
"Hitches: 2–4 (> 2× median) worst 3432–4415 ms" in the Game view's Stats box:

    MISTY_SCREENSHOT=<png> MISTY_SCENE=Assets/Scenes/MeadowDemo.scene MISTY_PLAY=1 MISTY_STATS=1 \
    MISTY_CAPTURE_FRAME=400 target/release/misty.exe <project>

## What the "worst" frame was

In a capture run the scene opens and Play starts inside `EditorApp::start`, before the first
editor frame, while the launcher's splash is on screen; the first frame then renders the scene
for the first time. All of that is one frame, and the Stats box's first sample (egui's
`stable_dt`) is that frame. So the 3.4–4.4 s was the **load frame**: project open + scene load +
entering Play + the scene's first render. Behind it were 2–3 more frames of 200–600 ms (tree
textures arriving) — the other "hitches".

It was 3.5–5.7 s on a **new build** of the editor and ~1.2 s on a second launch of the same
build: NVIDIA's driver shader cache is per executable, so the first launch of every build
compiles every pipeline from scratch. That is the developer's normal case (rebuild, run), and
why the user saw 3–4 s.

## Measuring

Capture runs (editor, release) gained:

- `MISTY_PROFILE=<file.jsonl>` records the profiler from the first frame (as the player has),
  including the scopes of `EditorApp::start` (they land in frame 0). New scopes: project open,
  editor scripts, the views, terrain layers, trees, grass, volumetrics and each pipeline made by
  `pipelines::build` (`Pipeline: <name>`). The script host, runtime and Play start had
  temporary scopes for this work (the numbers below); they were taken out again, that code
  being the networking branch's.
- A frame report on stderr: every editor frame over 100 ms ("slow frame N: X ms (ends T ms
  after start)"), the first editor frame's time since the process started, the worst frame, and
  "interactive" = the end of the last frame over 100 ms; "scene complete" = the last frame any
  view was still streaming in (grass chunks, terrain textures, trees). Times are from `main`'s
  first line.
- `MISTY_PLAY_AT=<frame>` presses Play on that frame (the toolbar's path, `Action::TogglePlay`)
  and reports the worst frame since.
- `MISTY_OPEN_SCENE_AT=<frame>:<scene>` opens a scene in the running editor (File ▸ Open
  Scene's path) and reports the worst frame since.
- `renderer: <adapter> (<backend>)` (Vulkan on this machine).

Three scenarios, all with `MISTY_TAB=Game MISTY_STATS=1`:

- **A** — the reported command: MeadowDemo opened and Play entered at start.
- **B** — MeadowDemo opened at start, Play pressed at frame 200.
- **C** — the editor on SampleScene, MeadowDemo opened at frame 100, Play at 300.

Three cache states:

- **cold** — no `Library/` and no generated meta files (a fresh checkout), exe copied to a new
  path first (a fresh driver shader cache).
- **rebuilt** — `Library/` kept, exe copied to a new path (a new build of the editor).
- **warm** — `Library/` kept, the same exe as the run before.

The "before" exe is main (6b313ad) with only the capture reporting added (the editor's
`capture.rs`, `main.rs`, `app.rs`); before and after runs were interleaved. Other agents ran
builds and GPU tests on the machine meanwhile, so single runs vary (load frames up to 2–3×
in a busy moment); the tables give two runs each.

## Where the time was (before)

Load frame, scenario A, warm (1.5–1.9 s of scopes), and what followed:

| | ms |
|---|---|
| Asset GUID scan (project open) | 38–43 |
| `EditorState::new` (project open) | 105–165 |
| Entering Play: `AssetSystem::auto` + `AssetSystem::for_editor` (each read every meta file under `Assets/`), string tables (a walk of `Assets/`), script host, physics | 150–180 |
| `Renderer::with_queue` (core pipelines) | 108–116 |
| Editor scripts' first load: a walk of `Assets/` for `.lua` files, a Lua state with the game API (another meta-file read and walk) | 135–150 |
| **Terrain layer arrays**: decode, resize and mip three 4-layer 1024² arrays on the render thread | **370–440** |
| Grass atlas: PNG decode + coverage mips | 41 |
| Tree pipelines (9) | 65–70 |
| Volumetrics: froxel / shaft pipelines, blue noise (void and cluster) | 38–56 |
| Grass visibility pipelines | 36 |
| Clouds, AO prepass pipelines, the rest | ~60 |
| *later:* **tree textures**: decode + mips of 6 textures per tree, 85–105 ms each, all 5 trees in 2 frames when the tree builds land | **200–440 per frame** |

On a new build the core and first-frame pipelines alone took ~2.5 s more (the terrain pipeline
800 ms, grass 250, trees 850, clouds 260 …).

Entering Play in a running editor (B, C): 140–190 ms — `AssetSystem::auto` 53 ms and
`AssetSystem::for_editor` 51 ms (each `GuidDatabase::read`: every meta file), the string tables'
walk of `Assets/` 25 ms, the script host 10–30 ms.

Opening MeadowDemo in a running editor (C): 610–630 ms warm, 2.4–2.6 s on a new build — the
same first-render work as above.

Also found: the editor scripts re-walked `Assets/` every 2 s on the main thread (scan for new
scripts: 50 ms in this project — a periodic hitch).

Not a cause: physics (184 trunk colliders: 11–20 ms), the Play scene copy (< 3 ms), Lua startup
of the game scripts (API registration, instantiation and Awake: < 5 ms), Misty GI and virtual
geometry (off: no time in their scopes), asset imports (none on this path), grass scattering
(already on worker threads since the perf-hitches round: 12 ms of chunk bookkeeping).

## Fixes

1. **Terrain layer arrays on a worker** (`render/terrain_layers.rs`): decoding and mip-mapping
   run on a thread; the terrain draws neutral placeholder layers (grey, flat, mid height) for the
   first few frames, then the arrays go up a layer at a time, about 8 MB per frame (writing
   all 64 MB at once stalled a frame for 150 ms). A changed texture rebuilds in the background
   while the old arrays draw.
2. **Tree textures on a worker** (`render/trees.rs`): the six mip chains of a tree are built on
   a thread; uploads are one tree per render (~25 ms). The previous version keeps drawing.
3. **Tree models cached on disk** (`trees/cache.rs`): `Library/TreeCache/<hash>.bin`, keyed by
   the recipe, the leaf and bark textures (path, size, modified time) and the generator's own
   source (`include_str!` of the tree modules: a code change is a new key). Editor only (tests
   build in place, built games have no Library). Trees are on screen by frame 6–10 of a warm
   launch instead of 50–70.
4. **Pipeline cache on disk** (`render/pipeline_cache.rs`): every pipeline is created through
   a `wgpu::PipelineCache` (Vulkan) opened from `Library/ShaderCache/<pipeline_cache_key>`;
   written 3 s after shaders compiled (on a thread) and on exit. A new build no longer
   recompiles every shader.
5. **Shader modules made once** per source (`pipelines::shader_module`) on the device the
   editor or player registers (`render::cache_shader_modules`; one device only: wgpu compares
   devices by id, and the many instances of the tests share ids): most pipelines share their
   source with others; parsing and validating the prelude plus source was 148 of the 218 ms the
   51 first-frame pipelines took.
6. **Prepared on worker threads as soon as the renderer exists** (`render/prebuilt.rs`,
   streaming only): the tree, grass visibility, grass cull, volumetrics and cloud pipelines,
   the cloud noise volumes; process-wide, the grass atlas mips and the blue noise. First use
   takes the result (waiting if the worker is not done).
7. **No walks of `Assets/` or meta-file reads on the main thread** where the GUID database
   (kept current by the asset watcher's thread) already knows the files:
   `asset_db::with_project` (asset groups: `AssetSystem::auto` / `for_editor`),
   `asset_db::assets_with_extension` (string tables), editor scripts' scan.
8. `RenderStats::loading` (terrain layers and trees still being prepared): the Scene and Game
   views keep repainting until they land, as for grass chunks.

Tests and screenshots (`set_grass_streaming(false)`) take the old in-place paths everywhere:
no placeholder, no prebuild threads, no disk caches, so renders stay identical.

## After

Worst frame after the load frame / frames over 100 ms / "interactive" (end of the last frame
over 100 ms, from process start) / Stats box "worst" (includes the load frame):

**A — open MeadowDemo and enter Play at launch (the reported command)**

| | load frame | worst after | > 100 ms | interactive | Stats "worst" |
|---|---|---|---|---|---|
| cold, before | 5.0–5.7 s | 600–980 ms | 1–2 | 6.7–8.4 s | 5726 ms |
| cold, after | 3.2–3.7 s | 40–50 ms | 0 | 4.2–5.9 s | 3674 ms |
| new build, before | 4.9–5.0 s | 616–625 ms | 2 | 6.7–6.9 s | 4957 ms |
| **new build, after** | **0.5–0.8 s** | **30–37 ms** | **0** | **1.5–2.7 s** | **509 ms** |
| warm, before | 1.2–1.3 s | 187–296 ms | 2 | 2.8–3.2 s | 1225 ms |
| **warm, after** | **0.33–0.38 s**¹ | **46 ms** | **0** | **1.0–1.2 s**¹ | **367 ms** |

¹ Two more warm runs made while other agents were building: load frame 1.1–1.4 s, everything
in it 2–5× slower, still nothing over 100 ms after it.

"Scene complete" (nothing streaming in any more): warm 2.9–3.3 s → 1.75–2.0 s; new build
7.2–7.6 → 2.2–3.4 s.

**B / C — entering Play in a running editor** (worst frame from pressing Play on):
before 140–192 ms in every state; after **13–28 ms** typically, 48–78 ms in runs where other
agents were building (one run measured 197 ms: a single 181 ms Lua `instantiate` stall in a
run where everything was 2–5× slower; 1–2 ms in every other run).

**C — opening MeadowDemo in a running editor** (worst frame from opening it):

| | before | after |
|---|---|---|
| cold | 2442–2627 ms | 37–68 ms |
| new build | 2476–2514 ms | 35–60 ms |
| warm | 613–631 ms | 37–64 ms |

After merging main (round 15 networking, virtual shadow maps; VSM is opt-in and its mark
pipeline goes through the pipeline cache too) the same runs give: load frame warm 0.32–0.49 s,
new build 0.50–0.53 s, cold 3.3–3.8 s; worst frame after it 35–60 ms in every state; entering
Play 13–18 ms; opening MeadowDemo 36–40 ms.

The cold load frame (3.2–3.7 s) is the first launch ever on a project with no `Library/` and
a build the driver has never seen: the core pipelines (`Renderer::with_queue`, 1.5 s with the
terrain pipeline alone at 0.8 s) and the first frame's mesh/AO variants compile in the driver.
The splash is on screen during it. Every later launch, new build or not, reads the pipeline
cache.

## Not done / left

- **Project open** is now most of the load frame (warm ~0.33 s): `EditorState::new`
  105–165 ms, the asset GUID scan 35–45 ms, `Renderer::with_queue` ~85 ms (core pipelines,
  ~15 ms each for terrain and grass even with the caches: naga's translation still runs).
  The splash covers it; making the core pipelines asynchronous would need the views to draw
  nothing until they exist.
- The **first-ever launch** (cold) still compiles the core pipelines on the main thread.
- The Stats box's history includes the load frame (its first `stable_dt`), so its "worst" is
  the project open time; the frame report of a capture run separates the two.
- The frame after the load frame can take 60–100 ms with nothing scoped in it: the GPU
  catching up on the first submits (presentation waits). Not measured further.
- Tree uploads are ~25 ms each (6 textures with mips, written whole); splitting them per
  texture would bring them under 10.
- The cloud noise volumes (a few MB of VRAM) are made at renderer creation in the editor and
  player even for scenes without clouds.
- The player gets the worker-thread changes (it streams) but no pipeline or tree cache (no
  `Library/`): a per-user cache folder would serve it.
