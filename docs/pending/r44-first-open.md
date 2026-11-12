# r44: first open of a project and shader compiles off the main thread

Branch `r44-first-open` (from main 9320362d). Target `E:\MistyTargets\r44-first-open`,
TMP/TEMP `C:\MistyTry\r44tmp`, editor runs on copies under `C:\MistyTry\r44\`.

## What was measured before (main 9320362d)

Fresh copy of SampleProject, `Library/` removed, `MISTY_WINDOW_SIZE=1100x650`,
`MISTY_SCREENSHOT` capture run with `MISTY_PROFILE`; each run with a freshly named copy of the
exe (NVIDIA's driver cache is per executable, so a new name is a cold driver cache, as for a
user's first launch of a new build). RTX 4060, Vulkan. Two other agents' builds were running.

| Run | First editor frame | Main-thread frame 0 | Of it |
| --- | --- | --- | --- |
| SampleScene, run 1 | 12.3 s | 11.5 s | `Renderer.New` 7.2 s (36 pipelines: terrain shade 2.5 s, terrain 2.3 s, grass 1.4 s), scene render 3.2 s (`Pipeline: mesh` 2.5 s), asset DB 0.3 s, state 0.3 s |
| SampleScene, run 2 | 10.2 s | 9.4 s | `Renderer.New` 5.7 s, scene render 2.7 s |
| SampleScene, run 3 (new exe name) | 8.7 s | 8.0 s | `Renderer.New` 5.4 s, scene render 1.8 s |
| MeadowDemo | 10.3 s | 9.6 s | `Renderer.New` 2.7 s, scene render 6.0 s (`Render.PrepareAssets` 3.9 s, two `mesh` variants 2.9 s) |

All of it before the first frame: the window showed the splash and then nothing (Windows:
Not Responding). After the first frame no frame was over 100 ms in these scenes except the
second meadow frame (313 ms). Texture block compression was already on a worker
(`sprites::compress::Mode::Background`); texture decodes, mip chains and model imports were
not.

## Changes

See "Round 44" in `docs/progress.md`.

## Measured after

Same setup, final build (47849323). "Worst frame" is the longest frame after the first one
(the capture report); before, the whole load was one blocked stretch before the first frame,
so the worst main-thread stall is the first column.

| Run | First editor frame | Worst main-thread frame | Frames > 100 ms | Scene complete |
| --- | --- | --- | --- | --- |
| Before, SampleScene (3 runs) | 8.7 / 10.2 / 12.3 s | 8.0 / 9.4 / 11.5 s (frame 0) | 0 after the first | (at first frame) |
| After, SampleScene (3 runs) | 1.09 / 1.22 / 1.36 s | 149 / 173 / 183 ms (`Editor.RefreshAssets`) | 1 | 6.1 / 7.8 / 8.9 s |
| After, `first_open` editor test | 3.4 s (busy machine) | 92 ms | 0 | 8.8 s |
| Before, MeadowDemo | 10.3 s | 9.6 s (frame 0), then 313 ms | 1 after the first | still streaming at 23 s |
| After, MeadowDemo | 1.18 s | 451 ms (the views' first frame) | 2 (+ the screenshot frame) | 17.8 s |

Main-thread frame 0 after: 0.44-0.46 s (`Editor.State` 0.22 s, `Editor.AssetDb` 0.09 s,
`Editor.ProjectBrowser` 0.04 s). The renderer is ready on its worker 5-8 s after start
("Renderer ready in … ms" in the Console); until then the Scene view shows "Compiling
shaders…" and the status bar the count. Median frame time during the load 1.2-2.0 ms. The
editor screenshots after loading match the ones before (only the Console line and noise
differ); golden shots 20/20.

Intermediate MeadowDemo runs showed what the later commits fixed: the views' first frame
waited 3.5 s for the tree pipelines (`Prebuilt.Wait`: now waited for on the renderer worker,
skipped with background loads), 0.6 s for the grass visibility pipelines (same), 0.9-1.5 s
for the screen-space and Misty GI set-up (now in the warm-up frame), 2.4 s behind a texture
upload on a below-normal import worker (now at normal priority) and 2.4 s decoding a
read-ahead texture on the main thread (now on the import queue). An 80 s delay at exit seen
once did not come back in later runs.

## Checks (47849323; the code as tested, docs added after)

- `cargo fmt --all --check`: clean.
- `cargo clippy --workspace --all-targets --release`: 0 warnings.
- `cargo test --workspace --release --no-fail-fast`: editor 915 passed, engine 1495 passed
  and 1 failed, the rest passed. The failure:
  `render::vgeo::gpu_tests::auto_picks_one_phase_and_lit_for_a_few_rocks` (line 353, the
  Auto visibility choice from GPU counters read back a few frames late). Alone on this
  branch: failed twice. On unmodified main 9320362d (same target, same machine): failed
  twice, passed once. A flake from before this round (r40's notes saw it too).
- New tests pass: `load_queue::tests` (5), `render::background_load_gpu_tests` (2),
  `first_open_tests::the_capture_report_parses`, and the ignored
  `first_open_of_sample_project_keeps_drawing` (run by hand: 92 ms worst frame).
- Golden shots: 20 of 20 ok (largest mean 0.39, at most 0.03 % changed).

## Left

- `Editor.RefreshAssets` (150-180 ms) once about 1.5 s after opening: a folder the Project
  window lists changes then (not traced to which file).
- `Editor.State` (0.2 s) and the asset scan (0.09 s) still run before the first frame.
- Model imports (`models::model`) still load on the calling thread: many callers (draw
  cache bounds, static batching, physics, picking) need the mesh at once, so placeholder
  meshes there are a larger change. SampleProject's models cost about 10 ms each
  (`Import.Model: …` scopes); objects whose model is loading appear when it is there.
- Mesh variants with optional Standard features (features > 0) and blended surfaces are
  left out (no stand-in) until compiled: their group 2 layout differs.
- The depth prepass pipelines (off by default) still build in the frame.
- `Grass.FlowerAtlas` takes 50-70 ms in some MeadowDemo frames (not part of this round).
- The pipeline cache across runs exists on Vulkan only (wgpu has none on DX12 / Metal); this
  machine runs Vulkan.
- MCP screenshots and the Hub thumbnail may catch a view before its loads finish (editor
  `MISTY_SCREENSHOT` runs wait).
