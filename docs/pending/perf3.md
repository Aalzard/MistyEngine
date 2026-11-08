# MeadowDemo performance, round 3 (branch perf3)

The user: "a really significant increase in performance … keep working until you find something
that keeps making the engine faster and faster", keeping the quality as it is, using the
Lumen-like (Misty GI) and Nanite-like (virtual geometry) systems where they help.

Target: MeadowDemo as shipped (rocks, flowers, the new leaves, Misty GI on), 1920×1080, RTX 4060.
Rule kept throughout: **the picture stays the same**. Every change is either exact (same
samples written, only less work), exact up to float rounding, or (one case) a change in which
of two overlapping grass elements keeps a sample, measured at ≤ 0.002 mean levels. The golden
check passes against the unchanged references.

## Result

**GPU frame at 1920×1080, steady state** (`meadow_perf_times`, `MISTY_SETTLE=200`: the
camera still for 200 frames, then 60 timed; before = main as this round started (af3f36d;
main's later virtual geometry merge leaves MeadowDemo's defaults alone), after = this branch,
the two interleaved in two rounds under the GPU-timing lock; both rounds within 0.1 ms):

| view | Ultra | High | Medium | Low |
|---|---|---|---|---|
| wide (the player's start) | 24.05 → **21.06 ms** (42 → 47 FPS) | 17.97 → **14.97 ms** (56 → 67 FPS) | 13.25 → **11.19 ms** (75 → 89 FPS) | 9.08 → **7.57 ms** (110 → 132 FPS) |
| behind (turned to the trees) | 26.70 → **22.75 ms** (37 → 44 FPS) | 20.09 → **16.36 ms** (50 → 61 FPS) | 14.52 → **11.64 ms** (69 → 86 FPS) | 9.84 → **7.66 ms** (102 → 130 FPS) |
| editor's docked Game view size (1125×558, wide) | 14.45 → **12.35 ms** (69 → 81 FPS) | 10.00 → **7.92 ms** (100 → 126 FPS) | 6.50 → **5.33 ms** (154 → 188 FPS) | 4.24 → **3.64 ms** (236 → 275 FPS) |

That is **−12 to −23 %** GPU time on every level and view (High −3.0 / −3.7 ms, Medium
−2.1 / −2.9 ms), with the same picture. With 60 settle frames (a camera that just stopped,
Misty GI not settled yet) the gain is about 1 ms less on High and Ultra.

Render CPU (`meadow_perf_cpu`, quiet machine): 1.4–2.2 ms before and after on every level and
view; the GPU sets the frame rate everywhere.

GPU ms per pass, wide view (before → after; several passes are now split or merged, so rows
group them):

| pass | Ultra | High | Medium | Low |
|---|---|---|---|---|
| Grass culling | 0.43 → 0.43 | 0.42 → 0.42 | 0.28 → 0.28 | 0.12 → 0.12 |
| Shadow maps (cascades, local) | 1.76 → 1.75 | 0.56 → 0.56 | 0.16 → 0.15 | – |
| Grass near shadow map (inner + outer) | 2.98 → 2.97 | 1.31 → 1.31 | 0.40 → 0.40 | – |
| Clouds | 1.73 → 1.72 | 0.54 → 0.53 | 0.54 → 0.53 | 0.35 → 0.35 |
| Screen-space prepass + GTAO | 1.11 → 1.11 | 1.08 → 1.08 | 0.90 → 0.90 | 0.38 → 0.38 |
| Shadow mask / denoise | 1.35 → 1.35 | 0.94 → 0.93 | – | – |
| Misty GI: probe trace | 1.07 → **0.17** | 1.06 → **0.17** | 0.21 → **0.06** | 0.10 → **< 0.05** |
| Misty GI: surface cache (now 1 frame in 4) | 0.22 → 0.22 / 4 | 0.21 → 0.21 / 4 | 0.20 → 0.20 / 4 | 0.11 → 0.11 / 4 |
| Misty GI: gather | 0.32 → 0.32 | 0.31 → 0.32 | 0.26 → 0.26 | 0.10 → 0.10 |
| GI compose (SSGI / SSR stage) | 0.35 → **0.26** | 0.35 → **0.26** | 0.33 → **0.24** | 0.24 → 0.24 |
| Sky | 0.22 → **0.05** | 0.20 → **0.05** | 0.21 → **0.05** | 0.21 → **0.05** |
| Terrain (depth + shading) | 3.37 → **2.59** | 3.06 → **2.37** | 2.78 → **2.16** | 2.46 → **1.96** |
| Trees, opaque meshes | 0.15 → 0.15 | 0.07 → 0.07 | – | – |
| Grass raster (depth + visibility) | 3.62 → **2.52** | 3.60 → **2.52** | 3.03 → **2.09** | 2.38 → **1.60** |
| Grass shading (classify + shade) | 1.70 → 1.69 | 1.67 → 1.66 | 1.58 → 1.56 | 1.39 → 1.37 |
| Grass composite | 0.44 → 0.47 | 0.44 → 0.47 | 0.44 → 0.46 | 0.42 → 0.45 |
| Main pass (rest) | 0.36 → 0.37 | 0.36 → 0.37 | 0.36 → 0.36 | 0.35 → 0.36 |
| Sun shafts (sharp + radial) | 2.37 → 2.37 | 1.15 → 1.15 | 0.89 → 0.89 | 0.24 → 0.24 |
| TAA | 0.47 → 0.47 | 0.47 → 0.47 | 0.47 → 0.47 | – |
| Bloom, tonemap, finish | 0.16 → 0.16 | 0.16 → 0.16 | 0.16 → 0.16 | 0.15 → 0.15 |
| **GPU frame** | **24.05 → 21.06** | **17.97 → 14.97** | **13.25 → 11.19** | **9.08 → 7.57** |

(Per-pass numbers are medians of the frames a pass ran in; the surface cache now runs one
frame in four, so its average is a quarter of the figure shown.) In the behind view the
terrain gains most: 3.22 → 1.64 ms on High (2.99 → 1.57 Medium), since the trees and the
blades in front of it now hide it before it is shaded.

**In the editor** (release build, MeadowDemo in Play mode, docked Game view, VSync off,
median of 300 frames after 600; the same editor build with `MISTY_FAST_OFF=all` (the four
switchable shortcuts off) against defaults; `perf3_editor_high.png` is the High run):

| level | shortcuts off | **this branch** |
|---|---|---|
| Low | 5.09 ms (196 FPS) | 5.16 ms (194 FPS) |
| Medium | 6.76 ms (148 FPS) | **6.04 ms (166 FPS)** |
| High | 10.35 ms (97 FPS) | **8.85 ms (113 FPS)** |

(The changes without a switch, 4, 5 and 8, are in both columns, so this understates the
branch a little. Low in the editor is bound by the editor's own frame, not the
GPU: the headless Game-view-size render takes 3.6 ms there.)

## Measuring

- `render/meadow_perf_tests.rs` (ignored, release; `cargo test --release -p misty_engine --lib
  meadow_perf_ -- --ignored --nocapture --test-threads=1`):
  - `meadow_perf_times` / `meadow_perf_matrix` / `meadow_perf_ab` as in round 2, plus
    `MISTY_SETTLE` (frames before timing: 60 by default; **200 for the steady state**, since
    Misty GI's levels settle after 128 updates), `MISTY_SETUPS` (matrix setups as options
    joined by `+`: `cover`, `msaa2`, `vsm`, `upq`, `vgeo`, `gioff`), `MISTY_AB=fast:<name>`
    (one shortcut off / on in one process).
  - `meadow_perf_cpu`: the profiler's scopes of `Renderer::render` per view and level.
  - `meadow_perf_shots`: `MISTY_SHOT_FRAMES` (48 like the golden shots), `MISTY_FAST_OFF`.
- **Every shortcut has a switch** (`render/fast_paths.rs`: `terrain_late`, `sky_late`,
  `gi_settle`, `grass_vis_depth`; all on). `MISTY_FAST_OFF=a,b` or `all` turns them off in any
  build (tests, editor, player), so each one was measured and look-checked against itself
  off in the same binary. The Frame Debugger keeps the old drawing order.
- New pass timer marks: `Screen-space prepass` (then `GTAO`), `Grass near shadow map (inner)`,
  `Sun shafts (sharp)`, `Main: terrain depth`, `Main: sky (late)`.
- **Renders are no longer bit-deterministic run to run** (Misty GI's mesh SDF bakes and the
  grass uploads finish on worker threads by time): two runs of the same build differ in
  ~100–5 000 pixels by 1 level (a few by up to 11). That is the noise floor of every look
  check below ("within noise": the same order of differing pixels, max a few levels).
- All timing held the shared GPU-timing lock; other agents' builds were running at times,
  which shows in CPU numbers (the first baseline read 11 ms of render CPU on the behind view;
  quiet, it is 1.7 ms).

## Baseline (before)

Main (af3f36d), 1920×1080, wide view (the player's start), GPU ms per pass (60 frames to
settle, so Misty GI had not settled; the steady-state "before" in the result is 17.97 ms on
High):

| pass | Ultra | High | Medium | Low |
|---|---|---|---|---|
| Grass culling | 0.44 | 0.42 | 0.28 | 0.12 |
| Shadow maps (cascades, local) | 1.77 | 0.56 | 0.16 | – |
| Grass near shadow map | 3.01 | 1.31 | 0.40 | – |
| Clouds | 1.74 | 0.54 | 0.54 | 0.35 |
| AO prepass + GTAO | 1.12 | 1.09 | 0.90 | 0.38 |
| Shadow mask / denoise | 1.36 | 0.95 | – | – |
| Misty GI (surface cache, probe trace, gather) | 1.60 | 1.61 | 0.67 | 0.31 |
| SSGI / SSR (the GI compose) | 0.35 | 0.35 | 0.33 | 0.24 |
| Main: sky | 0.22 | 0.21 | 0.21 | 0.21 |
| Main: terrain | 3.41 | 3.10 | 2.79 | 2.45 |
| Main: grass depth | 1.72 | 1.71 | 1.43 | 1.12 |
| Grass visibility (raster) | 1.92 | 1.91 | 1.60 | 1.26 |
| Grass shading | 1.72 | 1.68 | 1.58 | 1.38 |
| Main: grass composite | 0.44 | 0.44 | 0.44 | 0.42 |
| Main pass (rest) | 0.36 | 0.36 | 0.36 | 0.35 |
| Sun shafts | 2.39 | 1.16 | 0.89 | 0.24 |
| TAA | 0.47 | 0.47 | 0.47 | – |
| Bloom, tonemap, finish | 0.16 | 0.16 | 0.16 | 0.15 |
| **GPU frame** | **24.4** (sum) | **18.43** (54 FPS) | **13.73** (73 FPS) | **9.36** (107 FPS) |

Behind view (turned round to the trees): High 20.40, Medium 14.54, Low 9.83 ms (trees 1.23 ms
on High). Render CPU (quiet machine): 1.7–2.2 ms on every level and view.

## What changed, and what each saved

| # | change | exact? | saved (wide, 1080p) |
|---|---|---|---|
| 1 | Terrain drawn after the trees and the grass's depth (`terrain_late`) | same samples | terrain −0.54 ms High, −0.32 Low |
| 2 | Sky drawn last, on the far plane, depth tested (`sky_late`) | same samples | −0.10 to −0.15 ms every level |
| 3 | Misty GI: settled levels trace converged probes at 1/8 rate (`gi_settle`) | same after any change; steady state = same converged probes, refreshed less often | probe trace 1.06 → 0.17 ms, frame −1.35 ms High (steady) |
| 4 | Terrain: layer height in the normal array's alpha; zero-weight work skipped | exact (same texels; ulps where a mix by 1 is now the value itself) | terrain −0.21 ms High, −0.24 Low |
| 5 | GI compose: one bilateral upsample for irradiance and reflections | exact up to rounding | −0.09 ms High / Medium |
| 6 | Standing blades write the grass depth in the visibility raster (`grass_vis_depth`) | standing blades exact; a fan now always beats a standing blade behind it | grass −0.87 ms High, −0.62 Low |
| 7 | The lit cull lane holds only the fans (with 6) | exact | −0.24 ms High, −0.13 Low |
| 8 | Trees: bind groups and parameter buffer kept across frames | exact | CPU −0.01 ms (fewer per-frame allocations) |

### 1. Terrain after what hides it

The main pass drew the terrain first, then the trees, then the grass depth (alpha-tested
standing blades): every terrain pixel under dense turf ran the full layer and lighting shader
for nothing. Now the trees and the grass depth go first and the terrain after them, so its
fragments fail early depth where every sample of a pixel is covered. Same picture: the grass
depth writes only alpha, and every sample it wins is written again by the grass (the
visibility composite, or the lit grass in the fallback), which is how the old order's terrain
colour under those samples disappeared too. Main: terrain High 3.08 → 2.54 ms, Low 2.46 → 2.14.

### 2. Sky last

The sky was a full-screen pass at the start of the main pass, overdrawn by everything. It is
now drawn after the opaque scene (before anything that writes colour without depth: the lit
grass fallback, the grid, transparents) with its triangle on the far plane and a `LessEqual`
test, so only the samples nothing covers shade it: 0.20 → 0.10 ms.

### 3. Misty GI: probes update only where lighting changed

`misty_gi/mod.rs` tracks, per level, how many updates its shapes and placement (the level
hash, which includes its snapped origin) have stayed the same, and the same for a hash of the
lights and the sky. A level is **settled** when it, every coarser level (the interval merge
reads them), the lights and the sky have been unchanged for `SETTLE_FRAMES` = 128 updates (2 s
at 60 FPS; the probes have had 32–64 updates since the change and the bounces have
converged). In a settled level a probe whose cell has had 32 updates traces one turn in 8
(probes.wgsl: the others keep their history, decided per workgroup with
`workgroupUniformLoad`); with every level settled the dispatch is shrunk to exactly the probes
whose turn it is, and the surface cache relights one update in four. Any change (a light, the
sky, an object, the camera moving into new cells: the level hash changes) brings the full rate
back at once; new cells always trace (fewer than 32 updates).

- High, wide, steady state: probe trace 1.06 → 0.17 ms, GPU frame 17.55 → 16.20 ms (the
  surface cache's 0.21 ms now runs one frame in four).
- The golden shots (48 frames) never settle: they are unaffected by construction.
- Steady-state look (High, 400 frames, wide / close / oak, `gi_settle` on vs off): mean
  0.009–0.017 levels, max 2–4 levels, no pixel over 8 (two "off" runs: max 1–3 in a few
  hundred pixels). The settled probes hold the same converged values with a slightly
  different share of each ray rotation's noise; nothing a viewer can see.
- While walking the fine levels scroll (level 0 recomposes every 8 voxels = 50 cm at High),
  so the walking cost is the old one; the saving is for a still or slowly turning camera
  (most of a frame's time in an editor or a calm scene).

### 4. Terrain shading

A measured split of the terrain's 2.56 ms (High, wide, with 1; experiments since removed):
lighting (`shade`) ~1.2 ms, of which the sun's shadow factor and contact shadows 0.54, the
fog 0.12, the near grass map 0.06; the turf block under grass 0.40; the layers' second,
rotated copy 0.38; the macro variation 0.24; normal + height fetches 0.40.

- The height maps now live in the alpha of the normal array (`terrain_layers.rs`
  `with_height`: the mip chain filters every channel alike, so each mip's alpha is exactly the
  height map's mip): one fetch less per layer and projection, one binding and a third of the
  layer memory less.
- Work with exactly zero weight is skipped: the first copy where the blend noise saturates
  (`w == 1`), the mottle noise within 8 m (its weight `smoothstep(8, 50, d)` is 0), the worn
  spots past 25 m (`1 − mid` is 0).
- Main: terrain High 2.53 → 2.32 ms, Low 2.13 → 1.89 ms.

### 5. GI compose

The screen-trace compose (the only screen-trace stage in MeadowDemo) upsampled Misty GI's
irradiance and its glossy reflections with two identical bilateral upsamples (same size, same
neighbours, same weights). One `upsample_pair` now does both: 0.35 → 0.26 ms (High), 0.33 →
0.24 (Medium).

### 6. Grass blades rasterised once

With the grass visibility buffer the blades were rasterised twice: the grass depth pass in the
main pass (standing blades, alpha to coverage, depth writes) and the visibility raster (the
lit lane: standing blades again plus the flat fans, depth tested, no writes). Now:

1. the main pass's first part ends with the terrain's **depth only** (`terrain_depth`
   pipeline, the same invariant `vs_terrain`, no colour writes: 0.07 ms);
2. the visibility raster draws the standing blades (the depth lane, `vs_grass_vis_standing`)
   **with** depth writes and `LessEqual`, which is the grass depth pass's work and order, and
   keeps the last blade at the nearest depth exactly as the old depth-then-`LessEqual` pair
   did; then the flat fans (`vs_grass_vis_fans`) without writes;
3. after the resolve, the main pass's second part shades the terrain against that depth
   (`terrain_shade`: `LessEqual`, no writes) and draws the sky, then the composite.

The only difference: where a fan lies in front of the nearest standing blade, the fan now
always keeps the sample (before, a blade drawn after the fan in the lit lane's order covered
it). Measured against the shortcut off: mean ≤ 0.002 levels, ≤ 0.006 % of pixels over 8 levels
(High / Low × wide, close, behind, oak). Grass depth + visibility: High 1.73 + 1.91 → 2.75 ms,
Low 1.13 + 1.26 → 1.72 ms; GPU frame High 17.12 → 16.25, Low 8.47 → 7.85.

### 7. Fans-only lit lane

With 6, the fans pass still ran the vertex stage for every blade of the lit lane, only to
drop the standing ones. The culling now gives the lit lane a third mode (`lod.y = 2`): leave
out every blade that certainly stands (the reverse of the flat test with the same safety
margins; flowers too, whose flat fans never draw). If the visibility buffer then can't run,
the lit grass draws the depth lane's blades before the fans. Grass visibility High 2.75 →
2.51 ms, Low 1.72 → 1.60. Shots within noise. With Quality ▸ Grass Ground Cover (no fans at
all) the fans pass is skipped.

### 8. CPU

`meadow_perf_cpu` on a quiet machine: render CPU 1.4–2.2 ms on every level and view (Submit
0.7 ms, ShadowLists 0.1, Trees.Prepare 0.09). The first baseline's 11 ms on the behind view
was other agents' builds: `Render.ShadowLists` (a rayon loop) waited 4–15 ms for a core. The
trees made a uniform buffer and three bind groups per prototype every frame; they are kept now
(Trees.Prepare 0.094 / 0.103 → 0.084 / 0.080 ms). The render CPU is far below the GPU time at
every level; nothing else on it was worth a change.

## Golden shots

`cargo test --release -p misty_engine --lib golden_shots -- --ignored --nocapture
--test-threads=1` against the references on main, unchanged: every MeadowDemo shot (low,
medium, high, ultra × wide, close, behind, oak) **mean 0.00–0.01, 0.00 % changed**;
CyclesRoomDemo at every level mean 0.00. No reference was updated.

## Proposed defaults (not applied)

Measured with this branch (`meadow_perf_matrix`, `MISTY_SETTLE=200`, wide view, quietest of
2 interleaved rounds) and look-checked against the level's default (`meadow_perf_shots`,
1600×900, wide and close; mean channel difference / share of pixels over 8 levels).
Images (default | option | difference × 8): `docs/screenshots/gfx/perf3_upq_medium_wide.png`,
`perf3_cover_medium_wide.png`, `perf3_msaa2_medium_close.png`, `perf3_vsm_ultra_close.png`;
`perf3_visdepth_high_close.png` is shortcut 6 off | on | difference × 8 (black: within
noise).

| level | option | GPU frame | gain | look vs default |
|---|---|---|---|---|
| Medium | default | 11.34 ms (88 FPS) | | |
| Medium | **Upscaling ▸ Quality** | 6.94 ms (144 FPS) | **−4.40 ms** | 3.8–6.3 / 22–36 % (far grass softer; blade detail no longer matches pixel for pixel) |
| Medium | Grass Ground Cover | 10.60 ms (94 FPS)\* | −0.57 ms | 0.38–0.63 / 1.5–2.3 % |
| Medium | Upscaling Quality + Ground Cover | 6.57 ms (152 FPS) | −4.77 ms | |
| Medium | 2× MSAA | 9.51 ms (105 FPS) | −1.83 ms | 2.7–4.2 / 16–24 % (stipple, crisper turf: round 2's finding) |
| High | default | 15.13 ms (66 FPS) | | |
| High | **Upscaling ▸ Quality** | 9.84 ms (102 FPS) | **−5.29 ms** | 3.9–6.5 / 23–37 % (same) |
| High | Grass Ground Cover | 14.19 ms (70 FPS)\* | −0.72 ms | 0.32–0.53 / 1.2–1.8 % |
| High | Virtual shadow maps | 14.95 ms (67 FPS) | −0.18 ms | 0.05–0.30 / 0.1–1.1 % |
| High | Virtual Geometry (rocks) | 15.15 ms (66 FPS) | ±0 | 0.00 / 0.01 % |
| Ultra | default | 21.18 ms (47 FPS) | | |
| Ultra | **Virtual shadow maps** | 18.95 ms (53 FPS) | **−2.23 ms** | 0.33–0.34 / 1.0–1.2 % |
| Ultra | Virtual Geometry (rocks) | 21.28 ms (47 FPS) | ±0 | 0.00 / 0.01 % |
| Ultra | VSM + Virtual Geometry | 18.96 ms (53 FPS) | −2.22 ms | |
| Low | default | 7.69 ms (130 FPS) | | |
| Low | Grass Ground Cover | 7.13 ms (140 FPS)\* | −0.44 ms | 0.60–0.89 / 3.2–4.8 % (no TAA: speckle) |
| Low | Virtual Geometry (rocks) | 7.76 ms (129 FPS) | ±0 | |

\* Measured again after the last commit (the visibility raster no longer runs an empty fans
pass under the ground cover), against defaults of 14.91 / 11.17 / 7.57 ms in that run.

Recommendations (the user decides):

1. **Medium → Upscaling ▸ Quality** (67 % per side): the largest gain by far, 88 → 144 FPS
   at 1080p. The cost is the upscaler's known softness of the far grass (upscaler.md, "Image
   quality": wide view mean 6.1 levels, 76 % of native's edge contrast; trees, rocks and the
   horizon close to native). If that softness is acceptable on Medium, nothing else comes
   close.
2. **High → Upscaling ▸ Quality** only if 100 FPS on High matters more than the far grass's
   texture (66 → 102 FPS).
3. **Ultra → Virtual shadow maps**: −2.2 ms (47 → 53 FPS) for a small change (tree crowns'
   self-shadowing and the grass shadow band in the middle distance read slightly differently:
   `perf3_vsm_ultra_close.png`). On High VSM saves only 0.18 ms: not worth a change there.
4. **Grass Ground Cover** on High and Medium: −0.6 to −0.7 ms for a 0.3–0.6 mean change
   (round 2's band-fitted cover); not on Low (no TAA to hide the speckle).
5. **Virtual Geometry for the rocks** (High / Ultra): no gain and no change in these views
   (the rocks are a small part of the frame; vgeo-button's Auto paths made it equal to the
   LOD Groups). Neutral: turn it on for consistency with the conversion tool if wanted, not
   for speed.
6. **Not recommended**: 2× MSAA on Medium (−1.8 ms but the largest look change of all).

## Evaluated and not done

- **Async compute**: wgpu 30 exposes one queue per device (`Device::queue` is the only one;
  no compute-only queue in the API), and passes within a submission run in order with
  barriers between dependent passes. There is no way to overlap the grass culling, Misty GI or
  the GTAO compute with the shadow passes from wgpu today; the only lever is ordering
  independent passes next to each other, which drivers may overlap on their own. Not pursued.
- **Cached static shadow casters**: the cascades hold trees (swaying), grass (wind) and the
  terrain; only the terrain is static, and its cost in the cascades is small next to the
  grass casters'. The near grass shadow map is grass only (wind): nothing to cache.
- **Terrain far-field bake / cheaper distant LODs**: distant terrain is a small share of the
  terrain's pixels in these views (most of it is under grass near the camera, which 1 and 6
  removed); the measured split points at lighting, which a bake would not remove.
- **Half-rate GI gather**: 0.31 ms on High; temporal reuse of a per-pixel gather changes the
  look under motion. Not done.
- **Grass composite tiles** (tried, dropped): the classify pass listed the 8×8 tiles with a
  blade and the composite drew one quad per tile (draw_indirect) instead of a full-screen
  triangle. Same picture, no gain: 0.47 → 0.47 ms wide, 0.65 → 0.69 ms close (High). Almost
  every tile below the horizon holds a blade; the composite's cost is the per-sample writes
  where grass is.
- **Fewer near grass map texels on Ultra**: Ultra's outer slice at 4096² costs 2.3 ms (High's
  2048²: 0.65 ms). Lowering it changes the blade shadows between 4 and 12 m; not measured for
  look here, a candidate for the user to try on Ultra.

## Next ideas

- Sun shafts (sharp): 0.93 ms High, 2.16 ms Ultra; the march could stop at the first fully
  lit / fully shadowed run of cascade texels, or march at quarter resolution with the
  temporal accumulation (look change, needs the user's eye).
- The temporal shadow mask (0.93 ms High) and the terrain's shadow lookups overlap: terrain
  pixels whose prepass texel saw a grass blade fall back to the full PCSS filter.
- Misty GI while walking: toroidal scrolling of the clipmap (roadmap R5) would let scrolled
  levels keep their settled probes.

## Checks

Release profile only (build output on C:): `cargo fmt --all`; `cargo clippy --workspace
--all-targets --release`: 0 warnings; `cargo test --workspace --release --no-fail-fast`:
misty_editor 616 passed, 0 failed (66 ignored); misty_player 11 passed; misty_engine 985
passed, 1 failed, 74 ignored. The failure was
`misty_gi::mesh_sdf::tests::big_meshes_bake_in_the_background_then_read_back_from_the_cache`
(code this branch doesn't touch; it asserts on a process-wide bake cache that other tests
running in parallel also use): alone it passes, and a second full run of the engine's tests after the last change
passed with **986 passed, 0 failed**. The golden shots pass unchanged (above).

## Commits

- `8903587` harness: shortcut switches (`render/fast_paths.rs`), `MISTY_SETTLE`,
  `meadow_perf_cpu`, options, the screen-space prepass timer mark
- `62cb28f` Misty GI settled levels (3)
- `95bc59f` terrain layer heights in the normal alpha, zero-weight turf work skipped (4)
- `d412563` one bilateral upsample in the GI compose (5)
- `c767713` terrain late, sky late, standing blades write the grass depth (1, 2, 6)
- `e7558ec` tree bind groups kept (8)
- `21af0ba` timer marks for the near map's inner slice and the sharp shafts
- `7358995` fans-only lit cull lane; `MISTY_FAST_OFF` from the environment (7)
- `41b5899` look-check shots in a view target (Upscaling)
- `206c61f` no fans pass under Grass Ground Cover
