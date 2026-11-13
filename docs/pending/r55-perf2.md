# Round 55: performance round 2 (branch r55-perf2)

Goal: faster without changing the picture: the render's CPU cost (`Render.Submit.Finish`),
the NoonValley hero's GPU passes, memory, and the first run of a fresh project copy; plus two
first-frame stalls from bug hunt 3 (tree GPU-cull pipelines, flower atlas). Forked from main
09d55b5f (round 52 merged); main d97009b8 (bug hunt 3) merged in before the `warm_up` work.

Disks: target `E:\MistyTargets\r55-perf2` (binaries in `bin\`: `base.exe` / `misty-base.exe`
= main 09d55b5f, `misty-main.exe` = main d97009b8, `e.exe` / `misty-e.exe` = this branch),
TMP `C:\MistyTry\r55tmp`, editor runs on `C:\MistyTry\r55\SampleProject` (steady state) and
fresh copies `C:\MistyTry\r55\fresh_*` (first runs, deleted after), shots
`E:\MistyTargets\r55-shots`. Scripts in the session scratchpad `r55/` (round 52's,
re-pointed): `times.sh`, `stats.py`, `shots.sh`, `diff.py`, `ed.sh`, `prof.py`, `mem.sh`, and
new `first.sh` (editor on a fresh copy of SampleProject with a profiler capture, until "scene
complete") with `firststats.py` (settle time, frame times while loading, main-thread stalls).
Every timing under the `gpu-timing.lock`; RTX 4060, 16 threads; other agents' builds ran
during most measurements (the first-run numbers depend on that load more than on anything
here, see below).

Two measuring traps found on the way:
- The editor keeps one Scene view camera per project (`Library/EditorSettings.json`), so
  runs of different scenes in a row start from each other's views. `ed.sh` now deletes that
  file before each run (every run starts at the scene's Main Camera).
- Two binaries alternating on one project copy overwrite each other's wgpu pipeline cache
  (`Library/ShaderCache`): the first run after a switch recompiles shaders on background
  threads and looks slower (StreamingDemo "regressed" 2.3 → 3.9 ms that way). The final
  StreamingDemo / NoonValley editor numbers are three back-to-back runs of each binary.

## Changes

| change | commit | measured (before → after) | image |
|---|---|---|---|
| Frame recorded into five command encoders (split after shadow maps, clouds/fog, screen-space passes, main pass, TAA); each closed part finished on a rayon worker while the next records, submitted together | fc97554e | NoonValley High 1080p CPU render: hero 3.63 → 2.80, inside 3.00 → 2.09, down 3.62 → 2.61 ms; editor `Render.Submit.Finish` 1.7–2.6 → 0.15–0.7 ms (tables below) | none (same commands) |
| Shadow mask: 8-byte texels when it covers ≤ 2 lights (visibilities unorm16 × 2; depth as the rounded top 15 bits of its f32; frames; two radii) instead of 16 | 2e56d679, f99c1a23 | shadow mask hero 0.76 → 0.68, down 0.97 → 0.88, inside 1.54 → 1.50 ms | hero 0.001, down 0.076, inside 0.038 /255 vs base (base vs base: 0.001 / 0.254 / 0.163); no pixel > 8 (inside 0.01 %) |
| Background texture encodes off rayon's global pool, on their own pool of half the cores at normal priority | 896e5c26, 362c92b8, cb9b9ed2 | first runs: frames while loading NoonValley median 9.2 → 8.2 ms, p90 31.5 → 27.3; MeadowDemo 10.8 → 10.0, p90 24.7 → 23.0; settle time unchanged (23 → 24, 20 → 21 s) | none (same blocks) |
| Tree GPU-cull pipelines prebuilt on a worker with the tree pipelines; `warm_up` waits for both | af94f359 | first run `Trees.GpuCull` worst NoonValley 599–616 → 21–22 ms, MeadowDemo 418 → 62 ms; NoonValley's worst frame 800 → 278 ms | none |
| Flower atlas built and uploaded on its worker (was uploaded on the main thread) | fc0f30d6 | `Grass.FlowerAtlas` (11–187 ms on the main thread) gone | none |

Golden shots (20, e.exe, under the lock): all pass, MeadowDemo means 0.00–0.08, CyclesRoom
0.00. No golden image touched.

## CPU: `Render.Submit.Finish`

wgpu 30 records commands into a list and validates / tracks / encodes them in
`CommandEncoder::finish` (wgpu-core `encode_commands`). A finish takes only its own encoder's
lock and a read lock on the device's snatch lock, so separate encoders finish in parallel.
`render/frame_encoder.rs` closes the frame's encoder at five `mark` points and finishes each
closed part on rayon while the render thread records on; `finish` returns the parts in order
and they go to one `queue.submit`. Only the last part (weather, post, bloom) is finished on
the render thread. `FrameEncoder` derefs to the current `wgpu::CommandEncoder`, so no pass
changed.

Where to split (NoonValley hero, High 1080p, `noon_valley_times`, medians of 3 interleaved
runs, CPU render ms): one encoder 3.58; split after the screen-space passes 3.03; after
shadows, screen-space and main 2.75; all five points 2.70 (kept).

| `noon_valley_times`, High 1920 × 1080 (medians; base 8 runs, branch 2) | base CPU render | branch | base GPU sum of parts | branch |
|---|---|---|---|---|
| hero | 3.63 | **2.80** | 12.60 | **12.51** |
| inside | 3.00 | **2.09** | 11.50 | **11.47** |
| down | 3.62 | **2.61** | 11.73 | **11.65** |

Editor capture runs (1100 × 650, VSync off, 600 frames, last 400 profiled), main d97009b8 vs
this branch, `Render.Submit.Finish` and the whole `Render` scope (medians, ms) and whole
editor frames:

| scene (runs) | Finish main → branch | Render main → branch | frame median main → branch |
|---|---|---|---|
| NoonValley (3 back-to-back each) | 1.67–1.76 → 0.53–0.59 | 4.0 → 3.0 | 7.03–7.09 → 6.96–6.98 (GPU-bound: 6.5 ms GPU) |
| MeadowDemo (2 interleaved) | 2.59–2.63 → 0.72–0.74 | 5.14 → 3.54–3.79 | 8.73–8.78 → 8.63–8.74 (GPU-bound: 8.2 ms GPU) |
| StreamingDemo (3 back-to-back each) | 0.40–0.42 → 0.15 | – | 2.58–2.73 → 2.23–2.45 |

## GPU (NoonValley hero, High 1080p)

| idea | result |
|---|---|
| Shadow mask half-size texels (kept) | 0.76 → 0.68 ms |
| GTAO trace: one view-from-clip matrix per tap instead of inverse view-projection + camera basis (shader patch) | 0.50 → 0.50 ms: not ALU-bound, dropped |
| Shadow mask tile early-out | not tried: `sun_cascade` already returns after a blocker search that finds nothing, per pixel |
| Sun shafts: one coarser cascade / a downsampled map in the march | not done: changes the shafts (each step is already a single hardware PCF tap) |
| Sun shafts: half-resolution composite | not done: it adds into the full-resolution HDR image (its 0.15 ms is that bandwidth) |

## First run of a fresh project copy

Profiled with `first.sh` (a profiler capture of every frame, `Library` file times). There is
no Misty GI bake in NoonValley or MeadowDemo (no `Library/MeshSdf` is written). A fresh copy
waits for:

1. the first ~12–15 s: imports, virtual geometry builds, the scene load;
2. then the BC7 / BC1 encodes of the textures the renderer makes (tree maps and impostors,
   grass and flower atlases, terrain layers) and of imported ones, started on first use:
   NoonValley writes 65 entries (97 MB) to `Library/TextureCache`, MeadowDemo 51 (54 MB).

The encodes were `rayon::spawn`ed on the global pool, at normal priority on every core: the
frame's parallel loops (culling, tree prep) queued behind them and the render thread shared
its core, so every scope of the frame ran 3–4× slower while they ran. How long a first run
takes depends mostly on the machine's other load: the same base binary settled NoonValley in
127 s with two builds running, 30–52 s with one, 23 s on a quieter machine.

Final runs (main d97009b8 vs this branch, interleaved, one other agent's build running):

| scene | settle main → branch | frames while loading: median / p90 / p99 | `Render.Submit.Finish` median | worst main-thread stall |
|---|---|---|---|---|
| NoonValley run 1 | 23.4 → 24.2 s | 9.2 / 31.5 / 46.0 → 8.2 / 27.3 / 51.3 | 1.88 → 0.71 | 800 → 278 ms (`Trees.GpuCull` 599 → 22) |
| NoonValley run 2 | 23.3 → 24.6 s | 8.9 / 31.3 / 44.9 → 8.2 / 26.7 / 43.8 | 1.96 → 0.71 | 805 → 277 ms (`Trees.GpuCull` 616 → 21) |
| MeadowDemo run 1 | 19.9 → 21.2 s | 10.8 / 24.7 / 36.7 → 10.0 / 23.0 / 33.4 | 2.66 → 0.80 | 704 → 377 ms (`Trees.GpuCull` 418 → 62) |
| MeadowDemo run 2 | 19.8 → 21.2 s | 10.6 / 24.3 / 35.7 → 9.9 / 22.8 / 34.2 | 2.60 → 0.79 | – |

The remaining ~280 ms frames are the scene load (frame ~3900) and the screenshot frame at the
end, not loading work. The "per-frame budget" asked for was not built as such: the encodes
are whole-texture jobs on worker threads, and what made the editor slow while they ran was
where they ran (the frame's own thread pool, every core), not main-thread work; the
main-thread pieces of loading (tree uploads, one per frame already; the flower atlas, now on
its worker; the tree cull pipelines, now prebuilt) were the stalls.

## Memory (NoonValley hero, High 1080p, `noon_valley_memory`)

Unchanged: renderer allocations 460 MB, view target 213 MB (base and branch). Findings:

- **4× MSAA emission target**: 66 MB of the view target at 1080p (23 MB for the editor's
  1100 × 650 view). Dropping it in scenes without emissive surfaces needs
  a variant without the emission target for each of the 23 main-pass pipeline builders
  (meshes, trees, terrain, grass, water, VFX, custom shaders, virtual geometry...) or a sparse
  colour slot, and rebuilding them when emission appears. Not done this round.
- **Tree textures (160 MB "made textures" in `noon_valley_memory`)**: every tree texture
  role (bark, leaves, packed normal / roughness / occlusion / translucency, impostor albedo
  and normals) already maps to BC7 (1 byte per texel) when the device has BC and the source's
  Compression setting is on; BC5 + BC4 would be 1.5 bytes per texel for the packed maps (see
  `compress/generated.rs`). The mips are used (trees are seen from far away; the leaf and
  impostor mips keep alpha-test coverage). Not checked: whether the test harness's 160 MB is
  the compressed set (it runs without a project cache, encoding in place).
- **View target**: the largest items are the MSAA colour, emission and velocity, depth
  (all per sample). The temporal shadow mask (now 8 bytes per pixel for ≤ 2 lights) still
  allocates 16 bytes per pixel ×2 (66 MB at 1080p) so a third light needs no reallocation;
  allocating by the light count would save 33 MB when only the sun casts.

## Tried and dropped

- GTAO trace view-from-clip matrix (above).
- Background encodes on `background::pool()` (896e5c26: every core but one, below normal
  priority): fine on a quiet machine, but a build beside it starved the encodes (MeadowDemo
  settled after 427 s instead of ~50). Their own pool at normal priority instead.
- That pool at a quarter of the cores (362c92b8): NoonValley settled in 125–208 s instead of
  ~45 (busy machine); half the cores now (cb9b9ed2).
- Shadow mask depth as a half float (2e56d679): custom shaders are validated without the
  half-float capability and every custom shader material failed (StreamingDemo panicked on
  open; 18 tests failed); f99c1a23 keeps the top 15 bits of the f32 instead.

## Checks (on cb9b9ed2 + docs)

- `cargo fmt --all --check`: pass. `cargo clippy --workspace --all-targets --release`: 0
  warnings.
- `cargo test --workspace --release --no-fail-fast`: all pass: engine 1566 (111 ignored),
  editor 929, player 11, dedicated server 1, MCP end-to-end 1. (An earlier run on the
  half-float version of the shadow mask failed 18 custom-shader / shader-graph tests; fixed
  by f99c1a23.)
- Golden shots (20, under the GPU lock): all pass (MeadowDemo ≤ 0.08 mean, CyclesRoom 0.00).
