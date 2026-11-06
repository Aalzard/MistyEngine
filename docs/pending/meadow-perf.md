# MeadowDemo performance (branch meadow-perf)

User: "the frames are 30 FPS and on Low it's reaching 40, we need it way more, over 100 FPS".
Target: ≥ 100 FPS (≤ 10 ms) on Medium and Low, High as close as possible, without changing the
approved look (tag `meadow-look-1`).

## Measuring

- **GPU time per part of the frame**: `Renderer::set_pass_timing` (render/pass_timer.rs) writes
  timestamps between the passes of a render (and inside the main pass: sky, opaque meshes,
  terrain, grass depth, grass lit) and reads them back with a wait at the end of the render
  (profiling only; needs `TIMESTAMP_QUERY_INSIDE_ENCODERS` / `_INSIDE_PASSES`, now requested
  when the adapter has them).
- `grass_pass_times` (render/grass_gpu_tests.rs, ignored): MeadowDemo from the First Person
  Player's camera per quality level — median GPU frame time (frame timer, 60 frames after 60
  to settle), median CPU time of `Renderer::render`, and the median GPU time of every part.
  `MISTY_SIZE=1125x558` renders at another size (the editor's docked Game view on this
  machine), `MISTY_SCALED=1` at the level's Render Scale (as the editor and player do),
  `MISTY_GFX_SHOT=close` the close shot.
  `cargo test --release -p misty_engine --lib grass_pass_times -- --ignored --nocapture --test-threads=1`
- `grass_variants`: the same with one feature off at a time.
- `grass_shots` with `MISTY_QUALITY=high,medium,low` renders each level (1600×900, 48 frames,
  fixed wind clock; `MISTY_GFX_DIR` writes elsewhere). The harness now poses the camera that
  actually renders: the scene's own Main Camera is inactive since the player was placed, so
  the "close" shot used to be the same as the wide one.
- **In the editor**: `MISTY_SCREENSHOT=<png> MISTY_SCENE=Assets/Scenes/MeadowDemo.scene
  MISTY_TAB=Game MISTY_STATS=1 MISTY_PLAY=1 MISTY_CAPTURE_FRAME=600 MISTY_VSYNC=0
  MISTY_QUALITY_LEVEL=Medium misty SampleProject` plays the scene in the docked Game view
  (1125×558 here) and prints the median frame time of the last 300 frames.
- **Look checks**: every step's shots were diffed against the same shots of `meadow-look-1`
  (mean abs difference in levels, share of pixels off by > 8 / > 24 levels, mean luma per
  band of rows). Two renders of the same build already differ by mean 0.56 / 0.34 % > 8 (TAA
  noise); clouds and fog animate with the wall clock, so a slow run can move the clouds and
  their shadows (a "mean 4" diff that is all cloud shapes: re-render before judging).

## Before (meadow-look-1 + the timers, RTX 4060)

GPU ms per part, 1920×1080:

| part | High | Medium | Low |
|---|---|---|---|
| Shadow maps (cascades + grass casters) | 1.48 | 0.71 | – |
| Grass near shadow map | 1.95 | 0.81 | – |
| Clouds | 0.54 | 0.54 | 0.35 |
| AO prepass + GTAO | 1.64 | 1.08 | 0.47 |
| Shadow mask / denoise | 0.97 | – | – |
| Sky | 0.32 | 0.31 | 0.32 |
| Terrain | 2.61 | 2.40 | 2.05 |
| **Grass depth** | 3.49 | 2.85 | 2.05 |
| **Grass lit** | **12.00** | **9.51** | **6.30** |
| Rest of the main pass | 0.35 | 0.36 | 0.36 |
| Sun shafts | 1.17 | 0.91 | 0.24 |
| TAA | 0.50 | 0.50 | – |
| Bloom, tonemap | 0.18 | 0.18 | 0.17 |
| **GPU frame** | **27.3** | **20.2** | **12.5** |
| CPU (`Renderer::render`) | 4.7 | 3.3 | 1.5 |
| triangles | 34.0 M | 22.2 M | 9.5 M |

Variants (High): sun without shadows 19.1 ms (grass lit 12.6 → 8.3: the four cascade taps per
grass fragment cost ~4 ms), grass density 0.5 20.5 ms, no near grass map −1.6 ms; AO off,
volumetric fog off, TAA off, soft shadows off: each within ±0.5 ms.

**Present mode**: the editor window presented with `AutoVsync` on this machine's 60 Hz display.
The Game view could never show more than 60 FPS, and a frame that missed 16.7 ms waited for the
next refresh: High at ~27 ms GPU showed 30 FPS, Low 30–40.

**Where the grass time went**: a 16 m chunk draws the prefix of its key-sorted blades that its
*nearest* point keeps, so the chunks around the camera draw (nearly) all their blades, and the
vertex shader shrinks the ones past their own distance's share to height 0. Those blades do
not vanish: the card collapses onto its root as a small flat twisted fan, as wide as the
widened blade (up to ~12 cm). In the near chunks most drawn instances are such fans, and in the
middle distance they are a good part of the ground cover (see "Tried" below). They cost
~1.7 ms in the depth pass and ~2.5 ms in the lit pass at High.

## Changes

1. **Flat fans out of the passes that do not need them** (grass_blade.wgsl `grass_flat`): the
   grass depth pass (new entry `vs_grass_depth`), the AO / contact-shadow prepass and the grass
   shadow casters (cascades and near map) skip the blades the LOD shrank away; the lit pass
   still draws them, testing against the standing blades' depth without writing its own.
   Diff against meadow-look-1: within the noise floor (High wide mean 0.56 / 0.27 % > 8).
2. **Per-blade view culling in the vertex stage** (`grass_outside`): a blade wholly outside the
   view's side planes (and in front of its near plane for camera passes) by more than its card
   can reach puts every vertex on one point outside the view. Exact; small gain (it helps the
   chunks the camera stands in, half of which is behind it).
3. **Grass sun shadow taps share one cascade pick** (shadows.wgsl `sun_shadow_taps`): the four
   taps of `fast_shadow_factor` (a few cm apart) pick the cascade, blend band and fade once
   instead of four times (and no longer compute the soft-shadow noise mode 1 never uses).
   Meshes with fast shadows use it too.
4. **Blades carry their field values** (terrain/details.rs `Blade::field` / `slope`): the lush
   and dry patch weights, the swathe and the canopy relief and its slope at the root are worked
   out once by the scatter (it computed most of them already for the blade heights) instead of
   eleven value-noise fields per vertex per frame. `canopy_slope` is the CPU twin of
   `grass_canopy`. Blades grow from 48 to 72 bytes. Lit grass −1.0 to −1.4 ms at the editor's
   Game view size (vertex bound there), within noise at 1080p (fragment bound).
5. **Editor VSync**: the Game view's new VSync toggle (off by default, like Unity's Game view;
   saved in `hub.json` as `editor_vsync`, `MISTY_VSYNC=1/0` overrides) switches the window's
   present mode live (`eframe::Frame::set_wgpu_surface_config`). The player keeps its own
   VSync player setting.
6. Captures print the frame rate of their last frames; `MISTY_QUALITY_LEVEL` picks the level.

## Tried and dropped

- **Skipping the flat fans everywhere** (the lit pass too): High 27 → 21 ms, but the middle
  distance turns paler and speckled (+2 luma in rows 60–70 %, 3.8 % of pixels > 8 levels on
  Low): the fans are part of the approved ground cover.
- **Fans as a second draw of 4 / 6 vertices** (the root/tip or root/middle/tip rows of the
  card): 4 vertices −1.2 ms but fewer, smaller specks (2.9 % > 8 at High); 6 vertices looked
  right (1.0 % > 8) but saved nothing (the extra draw's skipped vertices cost what the fans
  saved). Fans as 3 rows inside the same draw: −0.3 ms at the editor size, 2.2 % > 8 on Low.
- **Fans with their own depth writes** (drawn lit between the depth pass and the blades):
  pixel-exact, but depth writes with alpha-to-coverage and discard run late Z: +7 ms.
- **Grass depth before the terrain** (the ground under the turf fails early Z): exact, but
  only −0.05 ms at the editor size, −0.3 ms at 1080p (thin blades leave a sample of most
  pixels to the ground); not worth reordering the Frame Debugger's events.
- Skipping the previous-frame blade (motion vectors): no measurable gain (the compiler shares
  everything but the wind), as the TAA branch found.
- Skipping the cloud term for grass under the horizon: not exact (the cloud image is traced
  at a lower resolution and blends across the horizon).

## After

GPU ms per part, 1920×1080 (same machine; other agents' builds shared the GPU, ±0.5 ms):

| part | High | Medium | Low |
|---|---|---|---|
| Shadow maps (cascades + grass casters) | 1.22 | 0.54 | – |
| Grass near shadow map | 1.48 | 0.75 | – |
| Clouds | 0.53 | 0.53 | 0.35 |
| AO prepass + GTAO | 1.48 | 1.05 | 0.44 |
| Shadow mask / denoise | 0.96 | – | – |
| Sky | 0.31 | 0.31 | 0.32 |
| Terrain | 2.59 | 2.39 | 2.05 |
| **Grass depth** | 1.82 | 1.47 | 1.10 |
| **Grass lit** | **10.52** | **8.59** | **5.48** |
| Rest of the main pass | 0.35 | 0.35 | 0.35 |
| Sun shafts | 1.16 | 0.90 | 0.24 |
| TAA | 0.50 | 0.50 | – |
| Bloom, tonemap | 0.18 | 0.18 | 0.17 |
| **sum of parts** | **23.2** | **17.6** | **10.6** |

At the editor's docked Game view (1125×558; Low at its 0.75 Render Scale, 843×418), GPU
frame: High 15.1 ms, Medium 10.9 ms, Low 4.5 ms (grass lit 5.9 / 5.1 / 2.5 ms, grass depth
1.4 / 1.1 / 0.7, terrain 1.1 / 1.0 / 0.6, near map 1.5 / 0.7 / –, cascades 1.3 / 0.5 / –).

**In the editor** (release build, MeadowDemo in Play mode, docked Game view 1125×558, median
of 300 frames; "before" = the same editor with meadow-look-1's renderer):

| | Low | Medium | High |
|---|---|---|---|
| before, VSync off | 6.3 ms (158 FPS) | 14.1 ms (71 FPS) | 21.2 ms (47 FPS) |
| before, VSync on (as the user had it) | 60 FPS (capped) | – | 48 FPS |
| **after** (VSync off, the new default) | **4.7 ms (212 FPS)** | **10.2 ms (98 FPS)** | **14.2 ms (71 FPS)** |

The editor is GPU bound throughout (Stats: render thread 1.5–1.7 ms; main thread frame time =
GPU time). Full screen at 1920×1080 with each level's Render Scale (a maximised Game view or
the player): Low 8.0 ms (125 FPS, 1440×810), Medium 18.2 ms (55 FPS), High 23.8 ms (42 FPS).

## Look

Before | after | difference ×8 per level, wide and close golden shots:
`docs/screenshots/gfx/meadowperf_{high,medium,low}_{wide,close}.png`; after next to the
reference photo: `meadowperf_vs_reference.png`; the editor in Play mode on Medium with the
Stats box: `meadowperf_editor_medium.png`.

| shot (vs meadow-look-1) | mean | > 8 levels | > 24 levels |
|---|---|---|---|
| High wide / close | 0.56 / 0.13 | 0.27 % / 0.06 % | 0.003 % / 0 |
| Medium wide / close | 0.13 / 0.15 | 0.06 % / 0.10 % | 0.002 % / 0.001 % |
| Low wide / close | 0.31 / 0.08 | 0.34 % / 0.30 % | 0.02 % / 0.03 % |
| same build rendered twice (High wide) | 0.56 | 0.34 % | 0.003 % |

No visible difference: the remaining pixels are single blade-edge samples (where two flat fans
overlap, the later one now shows instead of the nearer) and TAA noise.

## Left

- Medium at 1080p is still ~18 ms and High ~23 ms on the GPU: the lit grass is fragment
  bound (every blade is at least a pixel wide; four cascade taps, the near map, fog and the
  canopy shading per fragment) and the flat fans are a large share of it. Ways forward that
  change the picture a little and need the user's eye: a deliberate ground-cover layer under
  the grass (in the terrain shader) instead of the accidental fans (then skip them: −4 ms at
  High 1080p), finer draw prefixes per chunk (fewer fans), Medium at a 0.85 Render Scale, or
  honouring the quality levels' MSAA setting (Medium says 2×, Low off, but every level renders
  4× today).
- The harness's clouds and fog run on the wall clock (renders compared with each other can
  differ by the clouds alone).
