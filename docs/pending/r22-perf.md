# Round 22: performance (branch r22-perf)

Asked for: the best performance possible at the same visual quality, MeadowDemo first (the user
keeps asking for more FPS there); profile first, take the biggest costs one at a time, measure
every change before and after under the GPU-timing lock, keep the golden shots, commit each win
with its numbers; look at CPU frame time and scene-open time too; no default quality changes.

## Result

**GPU frame at 1920×1080, steady state** (`meadow_perf_times`, `MISTY_SETTLE=200`; before = main
at e43e4bc, after = this branch; the two binaries interleaved over two rounds under the
GPU-timing lock, mean of the rounds; every pair of rounds within 0.25 ms):

| view | Ultra | High | Medium | Low |
|---|---|---|---|---|
| wide (the player's start) | 19.56 → **18.42 ms** (51 → 54 FPS, −5.9 %) | 14.46 → **14.02 ms** (69 → 71 FPS, −3.0 %) | 11.09 → **10.89 ms** (90 → 92 FPS, −1.8 %) | 7.51 → **7.31 ms** (133 → 137 FPS, −2.7 %) |
| close (crouching in the grass) | 22.04 → **21.08 ms** (45 → 47 FPS, −4.3 %) | 17.28 → **16.84 ms** (58 → 59 FPS, −2.5 %) | 13.51 → **13.21 ms** (74 → 76 FPS, −2.3 %) | 9.30 → **9.04 ms** (107 → 111 FPS, −2.9 %) |
| behind (turned to the trees) | 20.94 → **19.00 ms** (48 → 53 FPS, −9.2 %) | 15.62 → **14.50 ms** (64 → 69 FPS, −7.2 %) | 11.52 → **10.93 ms** (87 → 91 FPS, −5.0 %) | 7.60 → **7.25 ms** (132 → 138 FPS, −4.7 %) |

Where it came from (ms, first round): terrain −0.17 to −0.36 on every level and view, trees
−0.85 (behind High) to −1.0 (behind Ultra), sharp shafts −0.88 (wide Ultra) / −0.15 (wide High)
/ +0.02 (Medium, see 3), grass shading −0.02 to −0.04. The look is the same: the golden check
passes against the unchanged references, and every change was compared at 1600×900 with its
switch off (or the shader line reverted) in the same binary (details per change below).

Render CPU: `Render.ShadowLists` no longer stalls (7.2 ms a frame on average, 30 ms frames, on a
busy machine before; < 0.02 ms now). The editor's Scene view is CPU bound (below), so GPU savings
show there only in Play mode's Game view and in the player.

## Profile (before)

MeadowDemo as shipped, 1920×1080, RTX 4060, `meadow_perf_times` with `MISTY_SETTLE=200`
(Misty GI settled), main at e43e4bc. GPU ms per pass (passes under 0.05 ms left out):

| pass | wide Ultra | wide High | wide Medium | wide Low | close High | behind High |
|---|---|---|---|---|---|---|
| Grass culling | 0.44 | 0.42 | 0.28 | 0.12 | 0.41 | 0.47 |
| Shadow maps (cascades, local) | 1.40 | 0.49 | 0.15 | – | 0.52 | 0.61 |
| Grass near shadow map (inner + outer) | 0.42 + 1.43 | 0.42 + 0.49 | 0.15 + 0.20 | – | 0.47 + 0.50 | 0.42 + 0.49 |
| Clouds | 1.74 | 0.54 | 0.55 | 0.35 | 0.36 | 0.45 |
| Screen-space prepass + GTAO | 1.10 | 1.08 | 0.90 | 0.37 | 1.57 | 1.21 |
| Shadow mask / denoise | 1.36 | 0.95 | – | – | 1.31 | 0.93 |
| Misty GI (surface cache, trace, gather) | 0.70 | 0.69 | 0.52 | 0.21 | 0.78 | 0.65 |
| SSGI / SSR (GI compose) | 0.26 | 0.26 | 0.24 | 0.24 | 0.33 | 0.26 |
| Main: trees (+ opaque meshes) | 0.15 | 0.06 | – | – | 0.16 | 1.49 |
| Grass visibility (raster) | 2.52 | 2.51 | 2.08 | 1.60 | 3.39 | 3.07 |
| Grass shading | 1.69 | 1.66 | 1.56 | 1.37 | 2.19 | 1.60 |
| Main: terrain | 2.52 | 2.29 | 2.07 | 1.87 | 2.71 | 1.60 |
| Main: grass composite | 0.47 | 0.46 | 0.46 | 0.45 | 0.65 | 0.41 |
| Main pass (rest) | 0.37 | 0.37 | 0.37 | 0.36 | 0.44 | 0.34 |
| Sun shafts (sharp) | 2.16 | 0.93 | 0.67 | – | 0.88 | 0.95 |
| Sun shafts (screen-space) | 0.22 | 0.22 | 0.22 | 0.24 | – | – |
| TAA | 0.47 | 0.47 | 0.47 | – | 0.50 | 0.46 |
| Bloom, tonemap, finish | 0.16 | 0.16 | 0.16 | 0.15 | 0.17 | 0.15 |
| **GPU frame** | **20.25** (49 FPS) | **14.99** (67) | **11.55** (87) | **7.85** (127) | **17.81** (56) | **15.56** (64) |

Render CPU (`meadow_perf_cpu`): 1.9–3.1 ms median on a quiet machine, but `Render.ShadowLists`
averaged 7.2 ms a frame on High and Medium (p90 frames 28–31 ms) while other builds ran.

In the editor (release, MeadowDemo, Scene view, `MISTY_CAMERA_BENCH=600` flythrough,
`MISTY_PROFILE` capture of frames 300–700) the frame is CPU-bound: 15.4 ms frame interval
(65 FPS) with the Scene view's GPU frame at 5.6 ms. Of it: `ui()` 12.4 ms (the Scene view's
`Renderer::render` 8.2 ms of that, `Render.Submit` 3.2 ms, panels' own UI 4.2 ms,
`Editor.GameFrame.Pre` 1.4 ms, grass uploads 1.3 ms), paint and present 3.0 ms.

## What the parts cost (shader patches)

`MISTY_SHADER_PATCH=<file>` (new, test builds only) rewrites shader sources before they compile,
so each experiment below is the same test binary with one patch, interleaved with the
unpatched binary under the GPU lock. Wide view, 1080p (ms):

| experiment | High | Low |
|---|---|---|
| Terrain: plain surface, full lighting | 2.29 → 1.09 | |
| Terrain: full surface, no lighting | 2.29 → 1.00 | |
| Terrain: neither (raster + depth test only) | 2.29 → 0.39 | |
| Terrain: no anti-tiling second copy | 2.29 → 2.06 | 1.85 → 1.61 |
| Terrain: no macro variation | 2.29 → 2.12 | 1.85 → 1.68 |
| Terrain: no grass tint (patches, swathes, mottle, worn spots) | 2.29 → 1.83 | |
| Terrain: no triplanar | 2.29 → 2.17 | 1.85 → 1.76 |
| Terrain: no height blend | 2.29 → 2.28 | 1.85 → 1.86 |
| Lit shaders: no contact shadows | terrain 2.29 → 2.16 | 1.85 → 1.74 |
| Lit shaders: no near grass shadow map | grass shading 1.66 → 1.60 | – |
| Sharp shafts: no cascade lookup | Ultra 2.15 → 0.75, High 0.93 → 0.73 | |
| Sharp shafts: no cloud shadow lookup | Ultra 2.15 → 2.13 | |
| Lit shaders: no cloud shadow lookup | terrain 2.29 → 2.24, grass shading 1.66 → 1.61 | |
| Lit shaders: no fog / clouds in front (`vfog_eval`) | terrain 2.29 → 2.17, grass shading 1.66 → 1.55 | 1.85 → 1.74, 1.36 → 1.27 |
| Lit shaders: no shadow map / mask lookup | terrain 2.29 → 1.92 | 1.85 → 1.81 |
| Lit shaders: no sky reflection colour | terrain 2.29 → 2.25 | |
| Lit shaders: no reflection probes | terrain 2.29 → 2.27 | |
| Trees: plain leaf colour, no lighting (behind view) | trees 1.24 → 0.26 | |
| Grass composite per pixel with a sample mask (one slot only: an upper bound) | 0.47 → 0.34 | 0.45 → 0.34 |

Material model v2 (round 21) against main before its merge (4e0425d, built from an archive of
that commit, same GPU lock, two interleaved rounds): every pass of MeadowDemo wide and behind,
High and Low, within ±0.01 ms (wide High 14.39 / 14.39 ms, behind High 15.52 / 15.50 ms,
opaque meshes 0.06 / 0.06 and 0.25 / 0.25 ms, trees 1.24 / 1.24 ms). Plain materials pay
nothing for the feature variants.

## What changed

| # | commit | exact? | saved (1080p) |
|---|---|---|---|
| 1 | Shadow lists on the render thread below 512 draws | exact | render CPU: `Render.ShadowLists` 0.4–1.3 ms → < 0.02 ms quiet, 13–19 ms → < 0.02 ms with builds running (median frame 8.6 → 2.8 ms) |
| 2 | `MISTY_SHADER_PATCH` (test builds) | tool | – |
| 3 | Sharp shafts read the cascade whose texels the march can resolve (+ thresholds from the CPU, 2048² maps and up) | ≤ 0.012 levels mean, max 9/255 | shafts Ultra 2.15 → 1.25 ms, High 0.93 → 0.77 ms, Medium 0.67 → 0.70 ms |
| 4 | Grass patches and worn spots skip the octave that cannot change them | same values | terrain −0.16 to −0.34 ms on every level |
| 5 | Trees: leaf depth first, each leaf sample shaded once | a few isolated leaf-edge pixels | trees −0.2 to −1.0 ms (behind High 1.25 → 0.41 ms) |
| 6 | Editor Profiler scopes per panel / Scene view stage / file poll | – | (diagnosis) |
| 7 | Clouds in front of geometry: skip the lookups nearer than the cloud base | exact (noise floor) | −0.02 to −0.06 ms (grass shading, terrain) |

### 1. Shadow lists without rayon (`render/frame_lists.rs`)

The culling and batching of every shadow map ran as a rayon parallel loop each frame. A job
injected from the render thread waits until a global-pool worker wakes up and is scheduled; on a
busy machine (a build, an asset import, the GPU driver's threads) that wait was most of the
frame: `Render.ShadowLists` averaged 7.2 ms a frame (p90 frames 28–31 ms) in the first CPU
profile. MeadowDemo has about 450 draws and 4–5 maps; serial, the lists take under 0.02 ms.
Frames under `PARALLEL_MIN` (512) draws now build them on the render thread, like draw prep.

### 2. Shader patches in test builds (`render/pipelines.rs` `patched`)

`MISTY_SHADER_PATCH=<file>` (blocks of `@@@` / text / `===` / replacement / `;;;`) rewrites the
WGSL of the mesh, terrain, grass, tree, post, cloud, froxel, sharp shaft, Misty GI, screen-space,
shadow-mask and culling shaders before they compile, in the lib's test binary only (release
builds compile the identity). Every "what the parts cost" number above is the same binary with
and without one patch, so no rebuild per experiment.

### 3. Sharp shafts: cascade by footprint (`volumetrics/sharp_shafts.rs`, `sharp_shafts.wgsl`)

The per-pixel shaft march (half resolution, 32 steps on High, 64 on Ultra) read the tightest sun
cascade holding each sample, through the hardware 2×2 compare. Without the lookup the pass took
0.75 ms instead of 2.15 ms on Ultra: 33 M scattered reads of 4096² cascades miss the cache. A
sample now starts its cascade search at the first cascade whose texel is at least half the march
texel's footprint at that distance (`FOOTPRINT_SHARE` 0.5; 1.0 and 2.0 measured the same, 0.25
kept 0.15 ms of the cost). Finer texels only alias at the march's spacing, which the temporal
accumulation averages away. The thresholds come from the plan's cascade texel sizes (the CPU
writes them into the shaft uniform), and only maps of 2048² and up use them: on Medium (1024²)
choosing cost more than it saved (0.67 → 0.75 ms when every thread worked the thresholds out
itself). A residual +0.02–0.03 ms stays on Medium (0.67 → 0.70 ms wide, 0.73 → 0.75 ms close:
the larger uniform and the skipped branch); High saves 0.15 ms and Ultra 0.9 ms. Switch:
`shaft_cascades`.

Look: `meadow_perf_shots` at 1600×900, Ultra and High, all four views: mean difference ≤ 0.012
levels, max 9/255, no pixel over 4 levels (two runs of the same binary differ by up to 3). The
golden shots drift exactly as much as before (0.15–0.28 mean, same as main).

### 4. Noise octaves that cannot change the result (`grass_field.wgsl`, `terrain/details.rs`)

`grass_patches` (lush and dry, three octaves each) and `grass_worn` (two octaves) end in a
smoothstep of a weighted sum of value noise. Each octave is 0..1, so once the octaves read so far
put the sum past either edge whatever the last one adds, the result is exactly 0 or 1 and the
last lookup is skipped. The terrain's ground tint reads both per pixel (the grass-tint block was
0.46 ms of the terrain on High); the CPU blade scatter (worker threads) uses the same functions
and gets the same early-outs. Terrain −0.28 ms wide High, −0.16 ms wide Low, −0.34 ms close High.

### 5. Tree leaves: depth, then shading once (`render/trees.rs`, `tree.wgsl`)

Leaf clusters overlap many times inside a crown and the lit leaves shaded every fragment that
passed the depth test (a plain leaf colour took the trees from 1.24 to 0.26 ms in the behind
view; drawing the trees nearest first saved nothing). The main pass now draws the leaves' depth
first (`fs_tree_leaves_depth`: the same coverage as `fs_tree_leaves`, alpha to coverage, only the
alpha written), then the trees with the lit leaves testing for equal depth (`leaves_equal`): each
sample is shaded once, by the leaf it shows, and bark behind leaves is not shaded at all. The tree
vertex position is `@invariant` so both passes produce the same depth. Switch: `tree_depth`.

Look: switch off against on, 1600×900, Ultra / High / Low, four views: mean ≤ 0.001 levels; 0 to
29 isolated pixels (leaf edges at exactly equal depth) over 4 levels, none over 16.

### 6. Editor: where its frame goes (`misty_editor` app.rs, panels)

Each dock tab has a `Panel: <title>` Profiler scope, the Scene view `SceneView.Gather`,
`SceneView.Gizmos`, `SceneView.WorldUi`, `SceneView.Overlays`, and the edit-mode pre-frame
`Editor.ReloadAssets` / `Splines` / `PollScripts` / `PollGuids` / `LiveReload`. MeadowDemo,
Scene tab, `MISTY_CAMERA_BENCH=600` flythrough with a `MISTY_PROFILE` capture (frames 300–700,
mean ms per frame, release build, machine shared with other builds):

| | ms |
|---|---|
| frame interval (median) | 15.6 (64 FPS) — GPU frame 6.0: CPU bound |
| `ui()` / paint + present | 12.5 / 2.8 |
| Scene view `Renderer::render` (of which `Render.Submit`) | 7.7 (3.0) |
| `SceneView.Gather` (cached draws, terrains, lights, environment, post) + draw cache | 1.3 + 0.5 |
| grass chunk uploads while flying | 1.3 |
| gizmo lines | 0.6 |
| live reload polling (all at once every 0.5 s) | 0.6 |
| Project panel / Hierarchy | 0.6 / 0.35 |

With other builds hammering E: the polling (stat calls of every watched asset on the main thread)
rose to 2.6 ms a frame on average, in 40 ms spikes twice a second.

### 7. No clouds in front of near geometry (`clouds/uniforms.rs`, `clouds_apply.wgsl`)

`clouds_at` (every lit fragment) sampled the cloud image and its distance layer and then hid the
clouds behind the fragment, completely up to 0.7 of their distance. Below the layer every
distance the trace writes (and the temporal resolve blends) is at least the climb to the cloud
base, since the base is a sphere around the planet's centre with the eye inside it. The apply
uniform now carries 0.6 of that climb (`clear_reach`, in `CloudApply.sun.w`); nearer fragments
skip both samples. Exact: same image (max 3/255 against the line reverted, the noise floor).

## Tried and dropped

- Trees nearest first in each instanced draw (the CPU sorts each frame): 0.00–0.1 ms, subsumed by
  the leaf depth pass.
- Grass composite per pixel with `sample_mask`: the single-slot upper bound saves 0.12–0.17 ms,
  but pixels with two blades need more passes; not worth the risk without a stencil.

## Checks

- `cargo fmt --all`; `cargo clippy --workspace --all-targets --release`: 0 warnings.
- Golden shots (`golden_shots`, final build, under the lock): all 20 within tolerance (MeadowDemo
  mean 0.15–0.39, changed ≤ 0.03 %; CyclesRoomDemo 0.00), the same drift as main.
- `cargo test --workspace --release --no-fail-fast`: engine 1332 passed (89 ignored), editor
  758 passed and 1 failed — `shader_graph_examples_render` (one of the known load-flaky tests;
  it passes run alone), player and integration tests passed.

## Look-changing options for the user (not done; defaults unchanged)

Measured or estimated from the profile above (1080p, wide view):

- **Ultra's outer near grass shadow slice at 2048² instead of 4096²** (Quality ▸ Grass Shadow
  Map stays High for the inner slice): the outer slice costs 1.43 ms on Ultra against 0.49 ms on
  High. Saves ~0.9 ms; shadows of single blades 4–12 m away get 12 mm instead of 6 mm texels.
- **Ultra clouds at a third of the resolution instead of half**: clouds 1.74 ms on Ultra against
  0.54 ms on High (quarter resolution). ~0.8 ms.
- **Ultra sharp shafts at 48 steps instead of 64**: the march is ~0.6 ms per 32 steps now.
  ~0.3 ms.
- **Anti-Aliasing 2× instead of 4× on Low and Medium** (round 2 already measured this as the
  biggest single option): every MSAA pass (grass visibility, terrain, composite, resolve).
- **Terrain anti-tiling copy off far away**: the second, rotated copy of each layer costs
  0.23 ms on High; fading it out past ~60 m would keep it where tiling shows.
- **Contact shadows off on the terrain past ~30 m**: 0.12 ms.

## What's left (same picture)

- Editor CPU (above): move the file polling off the main thread (or onto OS change
  notifications), cache the Scene view's per-frame scene queries (`SceneView.Gather` 1.3 ms)
  and gizmo lines, and look into `Render.Submit` (3 ms in the editor against 1.3 ms headless:
  more passes and uploads per frame, all validated at submit).
- Grass composite per pixel with a sample mask plus a stencil for pixels with two blades
  (upper bound −0.13 to −0.17 ms).
- Grass visibility (1.6–3.4 ms) and grass shading (1.3–2.2 ms) stay the biggest GPU costs; the
  terrain's lighting (shadow mask lookup, contact shadows, GI, fog) is ~1 ms of its 2 ms.
- Startup / scene open: the pipeline cache and streamed grass were done in earlier rounds; not
  re-measured here (the streaming round, r21-streaming, works on it in parallel).
