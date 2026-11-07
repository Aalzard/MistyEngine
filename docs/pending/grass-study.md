# Grass rendering study (branch grass-study)

User: "isn't there another, better method to render this much grass fast? Do a study, the
calculations, and test something." Same look as `meadow-trees-1` / `meadow-look-1`.

**Short answer.** Yes. The lit grass pass does not cost what it costs because of the
lighting: it shades every blade fragment that survives the depth test, at pixel rate per
*primitive* under 4× MSAA, in 2×2 quads: 15 M shader lanes for ~2 M visible blade samples. Shading each visible
grass sample once instead — a **visibility buffer** (rasterise a blade id, then light it in
one compute pass) — cuts the study's grass passes by 2.7–3.1 ms at the same look and the same
TAA stability with 4× MSAA (B4), 4.5–4.9 ms with 2× (B2, slightly more shimmer), and 5–6 ms
single-sample (B, same image but twice the shimmer). Projected on the live renderer (whose
lighting costs more per shade): **High 23.0 → ~17 ms, Medium 17.4 → ~11.4 ms (B2), Low 10.9 →
~8 ms at 1080p**. The industry staples (multi-blade clumps, shells/parallax far field, half-rate
shading, a lighting cache, temporal splitting) all lose to it here, or add little on top.
Recommendation and projected frame rates: section 4.

## 1. Calculations

RTX 4060, 1920 × 1080 (2.07 M pixels), MeadowDemo from the player's camera (wide), High
(density 1.0). Numbers from `study_calc` (CPU twin of the blade stage per distance band, and
fragment counters in the study's copy of the live shaders). The grass covers ~1.15 M pixels
(everything under the horizon, 55 % of the frame).

**Geometry per pass** (the same in the depth and the lit pass; the live renderer also draws
the casters, the near map and the AO prepass, which is why its frame counts 34 M triangles):

| band (m) | blades | standing | triangles | card width px (standing) | card height px | < 1 px | < 2 px |
|---|---|---|---|---|---|---|---|
| 0–5 | 54 k | 54 k | 0.43 M | 16.4 | 29 | 0 % | 0 % |
| 5–10 | 150 k | 112 k | 1.20 M | 9.2 | 14 | 0 % | 1 % |
| 10–20 | 289 k | 82 k | 2.31 M | 10.7 | 7.2 | 0 % | 0 % |
| 20–40 | 195 k | 53 k | 1.39 M | 5.3 | 3.6 | 0 % | 2 % |
| 40–80 | 73 k | 49 k | 0.44 M | 2.6 | 1.5 | 2 % | 28 % |
| 80–160 | 200 k | 159 k | 1.20 M | 1.25 | 0.6 | 32 % | 95 % |
| **total** | **0.96 M** | 0.51 M | **7.0 M** (9.0 M vertices) | | | | |

- 47 % of the drawn instances are **flat fans** (blades the LOD shrank to height 0, kept as
  ground cover): they cost vertices and, in the lit pass, overlapping fragments.
- A card is not a single blade: it is a clump texture of thin leaves, so "blades ~1 px wide"
  are leaves inside cards that are 5–16 px wide up to 40 m. Only past ~80 m are cards
  themselves around a pixel — 1.2 M triangles, but only ~3 % of the grass pixels. A thin-blade
  software rasteriser (C) therefore has little to take over at this density.

**Fragments** (counters; the counted lit pass forces early depth testing, like the timing
build does; the depth pass writes depth after `discard`, so its counts are before the depth
test):

| pass | invocations | 2×2 quads → lanes | pass the alpha test | MSAA samples covered |
|---|---|---|---|---|
| A grass depth (standing blades, 4×) | 11.0 M | 5.4 M → 21.6 M | 9.7 M | |
| **A grass lit (all blades + fans, 4×)** | **7.8 M** | **3.8 M → 15.3 M** (7.4 per screen pixel, 13 per grass pixel) | 6.6 M | 15.8 M |
| A1 lit (single-sample) | 3.6 M | 2.0 M → 8.1 M | 0.8 M | |
| visible blade samples, 4× (= B4's shades) | | | | 1.99 M (1.7 per grass pixel) |
| visible blade pixels, 1× (= B's shades) | | | | 0.77 M |

(close shot: lit 10.3 M invocations → 17.7 M lanes, visible 2.8 M samples / 1.2 M pixels;
behind: 8.6 M → 15.9 M lanes, 2.1 M / 0.8 M.)

So the lit pass launches **15.3 M lanes to colour ~2 M visible samples: 7.7× the work**,
from three sources: 51 % quad occupancy (thin leaves leave half of every 2×2 quad as helper
lanes), MSAA (a pixel touched by several blades is shaded once per blade: 7.8 M invocations
for 1.15 M grass pixels) and overdraw that early depth can't stop (the fans don't write depth,
so every fan in front of the ground shades, overlapping fans included; alpha-tested cards
behind the nearest one in other samples).

**Bytes per lit fragment** (study shader; the live one reads more): 2 atlas fetches
(RGBA8, mostly cache hits), 5 depth-compare taps (4 cascade + near map, 4 texels each),
cloud, AO and froxel fetches, ~20 B of MRT writes (colour, glow, velocity) per covered sample.
The pass is not bandwidth-bound: it is ALU/lane bound (the time follows the lane count).

**Floor**: shade each visible blade sample once. At the study's measured cost per shade
(0.32–0.49 ns in the resolve, blade re-evaluation included) that is 0.25 ms single-sample or
~1.0 ms at 4×, on top of a raster pass that writes depth + id (≈ the depth prepass the live
renderer already pays, 1.8–3.5 ms). The lit pass (5.3 ms in the study, 9.3–10.5 ms live) is
what can go: **up to ~80 % of the grass cost is removable (1×), ~50 % at 4×-MSAA quality.**

## 2. What was tested

The harness (`crates/misty_engine/src/render/grass_study/`, test builds only; the one change
outside it is the `#[cfg(test)] mod grass_study;` hook in render/mod.rs):

- `field.rs` builds the blades MeadowDemo draws for one camera exactly as terrain_grass.rs
  does (main a91ac1e): 16 m chunks inside the draw distance, the scatter level of each chunk's
  nearest point, the frustum test, each chunk's key-sorted draw prefix, coarse cards past 1.5
  LOD distances. One buffer for all, so a blade has one id.
- `study_blade.wgsl`, `study_field.wgsl`, `study_grass.wgsl` are copies of grass_blade /
  grass_field / grass.wgsl (shape, wind, LOD shrink + widening, fans, colour, canopy shade,
  coverage), so vertex and fragment work per blade is the live one.
- `study_common.wgsl` stands in for `shade_grass` (which needs the whole renderer's bind
  groups): hemisphere ambient + screen-AO fetch, the sun with 4 hardware-compared cascade
  taps (one cascade pick), a near grass-shadow tap, a cloud-shadow texture, light through the
  blade, GGX sheen, fog + a froxel fetch. **Every variant uses it**, so they compare fairly; it
  costs ~57–62 % of the live lit pass (study 5.28 ms vs live 9.28 ms on High with the same
  blades; depth passes 1.81 vs 1.82 ms) — the projections below scale shading by that.
- Ground: the terrain heights (1 m grid ±320 m) shaded with the grass colour. No sky,
  mountains, trees, clouds. Sun cascades (24 m with the blades as casters, 170 m ground only)
  rendered once per camera.
- One TAA for all (Halton jitter, 3×3 variance clip in YCoCg, 10 % blend, depth reprojection)
  and ACES tonemapping. Wind clock fixed (motion vectors = camera only).
- GPU time per pass: the engine's `PassTimer`. Each variant is timed 3–4 times interleaved
  with the others and keeps its fastest round (other agents' GPU jobs shared the GPU).

Variants (all at the live settings unless stated):

| | method | how |
|---|---|---|
| **A** | baseline | grass depth prepass (standing blades) + lit pass (all + fans), 4× MSAA + alpha to coverage, 3 MRTs |
| A1 | A single-sample | coverage → per-pixel per-frame noise threshold (TAA averages it) |
| **B** | visibility buffer | blades → depth + Rg32Uint (id, t, side) with a tiny alpha-test fragment; compute pass shades each pixel once: blade from the id, position from depth, card evaluated again at (t, side), `grass_surface` + lighting; 1×, stochastic coverage |
| **B4** | visibility buffer, 4× MSAA | the same at 4× (coverage → output sample mask, exactly what A2C does; ids as raw bits in R32Float + (t, side) in Rg16Float: core WebGPU can't multisample integer targets); the resolve shades each *distinct* blade of a pixel once and weights it by its samples |
| B2 | visibility buffer, 2× MSAA | as B4 with 2 samples (needs `TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES`) |
| C | B + compute raster | blades < 1.5 px wide skip the hardware raster; a compute pass walks their centre line (or a fan's across line) and keeps the nearest with a 64-bit `atomicMax(!depth << 32 \| id << 9 \| t)` (`SHADER_INT64_ATOMIC_MIN_MAX`), coverage as a noise threshold |
| D | clumps | past 1.5–3 LOD distances 3× fewer cards, each 3× wider with up to 12 leaves (the cards already are 4-leaf clumps) |
| E | far field | blades end at 40 m (AE: 60 m); the ground pass marches a procedural grass layer (10 steps, 4 cm blade grid, tuft hue) past it and shades the hit once |
| F | quarter-rate shading | B; each 2×2 block shades one rotating anchor first, the others copy it at similar depth, else shade |
| G | lighting cache | a world-space texture of sun visibility (cascades × near map × clouds) at 3 canopy heights, 2 clip levels (32 m @ 3 cm, 256 m @ 25 cm, 2 × 1024² RGBA8 = 8 MB), a quarter refreshed per frame; blades fetch it instead of 6 taps |
| H | temporal split | past 12 m each frame draws half the blades (hash of blade + frame parity), TAA accumulates; H2: the split blades 1.4× wider (coverage) |
| B4H, BE, BEG, AEG | combinations | |

## 3. Results

### GPU time (study, 1920 × 1080, RTX 4060, best of 3–4 interleaved rounds)

Wide shot, High (density 1.0, sun shadows). "grass + ground" = the grass passes plus the
ground pass (which changes with the sample count and with E's far field); everything else in
the study frame (TAA 0.2 ms, MSAA resolve 0.2–0.3 ms) is the same for all.

| variant | passes (ms) | grass + ground | vs A | look vs A: mean / > 8 levels | TAA shimmer (levels) |
|---|---|---|---|---|---|
| **A** baseline | depth 1.81, lit 5.28, ground 0.51 | **7.60** | – | – | 1.50 |
| A1 1× | depth 1.33, lit 3.36, ground 0.29 | 4.98 | −2.62 | 2.58 / 9.1 % (darker −1…−4) | 2.78 |
| **B** vis buffer 1× | raster 1.96, resolve 0.25, ground 0.31 | **2.52** | **−5.08** | 2.78 / 10.5 % (darker −1…−5) | **2.81** |
| **B2** vis buffer 2× | raster 2.61, resolve 0.56, ground 0.35 | 3.52 | **−4.53** ¹ | 2.01 / 5.5 % (darker −1…−3) | 1.85 |
| **B4** vis buffer 4× | raster 3.49, resolve 0.97, ground 0.43 | **4.89** | **−2.71** (−3.09 ¹) | **1.10 / 0.8 %** | **1.54** |
| C B + compute raster | raster 1.74, swr 0.31, resolve 0.24, ground 0.29 | 2.58 | −5.02 (B −5.08) | = B | 2.82 |
| D clumps ×3 | depth 1.57, lit 7.11, ground 0.51 | 9.19 | **+1.59** | 1.92 / 8.0 % | 1.55 |
| E far field from 40 m | depth 1.46, lit 4.55, ground 0.77 | 6.78 | −0.82 | 2.13 / 7.5 % (far band +13 brighter) | 1.51 |
| AE far field from 60 m | depth 1.52, lit 4.63, ground 0.74 | 6.89 | −0.71 | 1.59 / 5.6 % (+10) | 1.51 |
| F B at quarter rate | raster 1.85, resolve 0.40, ground 0.29 | 2.54 | −5.06 (= B) | 3.47 / 14.4 % | 3.56 |
| G lighting cache | update 0.08, depth 1.81, lit 4.40, ground 0.47 | 6.76 | −0.84 | **0.15 / 0.2 %** | 1.50 |
| H split past 12 m | depth 1.46, lit 4.20, ground 0.51 | 6.17 | −1.44 | 1.24 / 4.6 % (mid band −5.5) | 1.57 |
| H2 split, 1.4× wider | depth 1.63, lit 4.66, ground 0.51 | 6.80 | −0.80 | 1.22 / 4.2 % (−4.0) | 1.60 |
| B4H B4 + H2 | raster 2.96, resolve 0.96, ground 0.43 | 4.35 | −3.25 | 2.05 / 6.6 % (−5.9) | 1.66 |
| BE B + E | raster 1.63, resolve 0.23, ground 0.44 | 2.29 | −5.31 | 4.59 / 16.9 % | 2.76 |
| BEG B + E + G | raster 1.63, resolve 0.19, update 0.09, ground 0.39 | 2.30 | −5.31 | 4.60 / 16.9 % | 2.76 |
| AEG A + E + G | | 5.94 | −1.66 | 2.20 / 7.4 % | 1.51 |

¹ B2 was added later and timed in a second run with A, B, B4 again (A 8.04, B −5.69, B4 −3.09):
the numbers move ±0.4 ms between runs, the order never does.

Look columns: the last TAA frame (64 frames) against A's; "shimmer" = per-pixel temporal
standard deviation of luma over the last 16 frames, lower half of the frame, camera and wind
still (A's own value is the floor: TAA jitter on thin leaves).

**Other levels and shots** (grass + ground, saving vs A):

| | A | A1 | B | B2 ¹ | B4 | C | D | E | F | G | H | B4H |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| wide High | 7.60 | −2.62 | −5.08 | −4.53 | −2.71 | −5.02 | +1.59 | −0.82 | −5.06 | −0.84 | −1.44 | −3.25 |
| wide Medium (0.75) | 6.44 | −2.34 | −4.26 | −3.48 | −2.01 | −4.22 | +1.59 | −0.54 | −4.25 | −0.45 | −0.89 | −2.58 |
| wide Low (0.5, no shadows) | 4.79 | −1.93 | −3.22 | −2.21 | −1.50 | −3.07 | +0.59 | −0.53 | −3.08 | −0.30 | −0.95 | −1.76 |
| close High | 8.96 | −3.37 | −6.23 | −4.86 | −3.13 | −6.01 | +1.14 | −1.11 | −6.01 | −1.13 | −1.29 | −3.62 |
| behind High ² | 9.20 | −3.10 | −6.33 | −4.60 | −2.57 | −6.08 | +1.81 | −0.75 | −6.19 | −0.90 | −1.44 | −3.36 |

² Behind: 1.22 M blades / 8.9 M triangles per pass (the field around the player's back). Its
first run hit other agents' GPU jobs in every round for five variants; those were re-run.

### Motion (walk 1.4 m/s + turn 25°/s, 1.5 s, against A frame by frame)

| | A1 | B | B2 | **B4** | F | H | H2 | B4H |
|---|---|---|---|---|---|---|---|---|
| mean / > 8 levels | 2.51 / 9.1 % | 2.72 / 10.5 % | 1.97 / 5.6 % | **1.10 / 1.1 %** | 3.29 / 13.6 % | 0.96 / 3.1 % | 1.05 / 3.5 % | 1.90 / 6.1 % |
| frame-to-frame change (A 5.84) | 6.72 | 6.79 | 6.48 | 5.98 | 7.13 | 5.51 | 5.58 | 5.74 |

- **B4 stays at A** while moving (same as at rest): the method changes the cost, not the image.
- Single-sample (A1, B) and F flicker more (frame-to-frame change +15–22 %); F also smears
  per-blade colour within 2×2 blocks.
- **H** (temporal split): a low mean difference, but the split band (past 12 m) shows as
  sparse bright/dark speckle in the difference (`study_motion_H.png`) — half the blades pop in
  and out each frame and the TAA clamp lets part of it through — and it is 4–6 levels darker at
  rest (half-coverage averages to less than full coverage; 1.4× wider blades only halve that,
  H2). Ghosting is not visible in these runs (camera reprojection is exact here, the wind is
  frozen); with real wind the split blades' history would also be wrong where they moved.
- **G** with the clouds moving 8× faster than normal: mean difference to A 0.31 (0.15 at
  rest) — cloud shadows read right with a 4-frame refresh (they move ≤ 0.2 m per refresh).

### What each method does to the cost — and why

- **B / B4 (visibility buffer): the win.** The id raster costs about what the depth prepass
  costs (1.8–2 ms at 1×; 3.5 ms at 4× because it carries the fans too and writes two MSAA
  targets with a shader-written sample mask), and the shading collapses from the lit pass's
  5.3 ms to a 0.25 ms (1×) / 1.0 ms (4×) compute pass: 0.77 M / 1.99 M shades instead of
  15.3 M lanes. The quad waste, the MSAA per-primitive shading and the fan overdraw all go.
- **B2**: halfway on cost and look; it needs `TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES` (core
  WebGPU allows only 1 and 4 samples for these formats).
- **A1**: dropping MSAA alone saves half of what B saves, and costs B's shimmer: most of B's
  extra win over B4 is the sample count, not the method.
- **C (compute raster)**: no gain. Only 0.15 M blades (the 80–160 m band) are under 1.5 px,
  they cover ~0.16 M pixels; the compute pass costs 0.31 ms to save 0.22 ms of raster. Thin-
  primitive software raster pays off for real single-blade geometry (millions of 1-px
  strands), not for clump cards that are 5–16 px wide up to 40 m.
- **D (clumps)**: slower. The pass is fragment-bound, not vertex-bound: −21 % triangles, but
  3× wider cards (and fans) overlap more: lit lanes 15.3 M → 25.1 M.
- **E (far-field layer)**: −0.5…−1.1 ms on A, −0.2 ms on B. Little grass is that far in
  screen terms (blades past 40 m are 28 % of the instances but few pixels) and the layer costs
  its own shading in the ground pass. Not art-tuned (the far band reads 10–13 levels brighter
  and flatter than the blades).
- **F (quarter-rate shading)**: no gain once shading is only 0.25 ms (the workgroup sync costs
  more than it saves), and the worst shimmer/smear of all. Shading rate only pays when shading
  dominates — which is exactly what the visibility buffer removes.
- **G (lighting cache)**: −0.8…−1.1 ms on A (lit 5.3 → 4.4: six shadow/cloud taps → one
  fetch), visually identical (0.15 mean), 8 MB, 0.08 ms update per frame. On top of a
  visibility buffer it is worth ≤ 0.1–0.3 ms. Blade-shaped shadows close up keep only the cache's
  3 cm resolution at 3 heights; with moving blades the near-map shadows would lag up to 4 frames.
- **H (temporal split)**: −1.4 ms on A, −0.5 ms on B4, at a visible coverage loss and speckle.

## 4. Recommendation

**Replace the grass depth + lit passes with a multisampled visibility buffer: B4 on High and
Low, B2 on Medium** (B, single-sample, only if the user accepts ~2× the TAA shimmer — he
said he hates it, so not by default). Leave blade generation, LOD, culling, the AO prepass and
the grass shadow passes as they are. Skip C, D, F; keep G and E as later options only if
the forward path stays.

### Projected full-screen 1080p frame (RTX 4060, with trees)

Live renderer measured in this session with the same harness the meadow-perf branch used
(`grass_pass_times`, wide shot, 1920 × 1080): grass depth + lit = **11.10 / 9.10 / 6.21 ms**
(High / Medium / Low), frame (sum of parts) 21.9 / 16.5 / 10.1 ms. The study's lighting costs
less than the live one (study lit 5.28 / 4.46 / 3.18 ms vs live 9.28 / 7.63 / 5.11 with the same
blades; depth passes equal within 0.05 ms), so each projection keeps the raster time and scales
the resolve (pure shading) by live/study lit = 1.76 / 1.71 / 1.61:

| level | live grass now | B4 grass | B2 grass | B grass | frame now (your numbers / this session) | **with B4** | with B2 | with B |
|---|---|---|---|---|---|---|---|---|
| High | 11.1 | 5.2 (−5.9) | 3.6 (−7.5) | 2.4 (−8.7) | 23.0 / 21.9 ms (43–46 FPS) | **17.1 / 16.0 ms (58–63 FPS)** | 15.5 / 14.4 (65–69) | 14.3 / 13.2 (70–76) |
| Medium | 9.1 | 4.7 (−4.4) | 3.1 (−6.0) | 2.0 (−7.1) | 17.4 / 16.5 (57–61) | 13.0 / 12.1 (77–83) | **11.4 / 10.5 (88–95)** | 10.3 / 9.5 (97–105) |
| Low | 6.2 | 3.4 (−2.8) | 2.3 (−3.9) | 1.4 (−4.8) | 10.9 / 10.1 (92–99) | **8.1 / 7.3 (123–137)** | 7.0 / 6.3 (143–159) | 6.1 / 5.4 (164–185) |

(Low at its 0.75 Render Scale, as the player renders it, is ~0.56× the pixels again.) Low has
no TAA: B4's sample-mask coverage is then exactly today's alpha-to-coverage dither; B2 or B
without TAA would show their coarser dither, so Low gets B4.

The remaining High frame is then ~9–10 ms of non-grass passes (terrain 2.6, AO 1.5, shadow
mask 1.0, sun shafts 1.2, cascades 1.3, near grass map 1.5, …) + ~5 ms of grass. Further,
measured or estimated:

- The fans are an estimated up to ~1.5 ms of B4's 3.5 ms raster (47 % of the instances; the
  4× depth prepass without them costs 1.8 ms). A deliberate ground-cover term in the terrain
  shader instead of the accidental fans (already suggested by meadow-perf) could bring B4
  grass on High to ~4 ms (estimate, not measured) — a look change for the user to approve.
- The near grass shadow map (1.5 ms) and grass cascade casters are untouched by all of this.
- With B (1×) the whole main pass could go single-sample (terrain 2.6 ms → ~1.6), not counted.

### Effort

- Visibility raster in the main pass's depth (4×, two extra MSAA targets: id as raw bits in
  R32Float, (t, side) in Rg16Float; `fs_vis4` is ~20 lines) replacing "grass depth"; the
  resolve as a compute pass after the MSAA resolve, writing colour, glow alpha and velocity
  for grass pixels into the single-sample targets (it reads the MSAA colour for the
  non-grass samples, like `cs_resolve4`).
- `shade_grass` and the fragment half of `fs_grass` callable from compute: the lighting bind
  group layouts (view, lights, shadows, screen-space, clouds, fog, near map) need `COMPUTE`
  visibility; implicit-derivative samples become `textureSampleGrad` with the card's pixel
  footprint (done in the study).
- A global blade id: after meadow-perf2 the culling writes the instance lists; the id is the
  index into that pooled list (`instance_index` incl. `first_instance`, already used).
- Velocity: the resolve evaluates the blade again at the previous wind time (one more
  `grass_blade_ts` per shade).
- Frame Debugger events and pass-timer labels; golden-shot diffs against meadow-look-1.

Estimate: 3–5 days for B4 + B2 in the live renderer including tests, given meadow-perf2's
culling has landed.

### Risks

- **Portability**: B4 uses core WebGPU only (4× MSAA R32Float/Rg16Float render targets, a
  fragment `sample_mask` output, compute `textureLoad` of multisampled textures) — fine on
  AMD, Intel and Apple (Metal supports sample-mask output and MSAA on these formats). Integer
  targets can't be multisampled in core WebGPU (tested: Rg32Uint and R32Uint refused), hence
  the float-bits trick; ids keep bit 30 set so they are never denormal/NaN. B2 needs
  `TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES` (2 samples): fall back to B4 where missing (the
  same applies to the quality levels' "2× MSAA" setting today). C needs 64-bit storage atomics
  (DX12 SM 6.6 / `shaderBufferInt64Atomics`; missing on older Intel and on Metal) — another
  reason to drop it.
- **Memory**: B4 adds 2 × 4 samples × 4 B = 32 B per pixel = 66 MB at 1080p (B2 33 MB, B 17 MB);
  G 8 MB.
- **Bandwidth-weak GPUs** (integrated): the 4× id raster writes 2 MSAA targets; B2 is the
  natural fallback there.
- Transparent passes drawn after the grass inside the main pass would have to move after the
  resolve (their depth test still uses the MSAA depth).
- The study's projection assumes the live shading cost per shade scales like its lit pass;
  the real gain should be measured right after the integration (the harness patterns are here).

### How it plugs in after meadow-perf2 merges

1. Keep meadow-perf2's blade path (scatter, LOD prefixes, GPU culling, instance lists) as the
   input; only the two main-pass grass draws change.
2. `render/terrain_grass.rs`: draw with a new `grass_vis` pipeline (vertex = `vs_vis`: the
   live `grass_vertex` minus the colour work, outputs id + card point) instead of `grass_depth`
   and `grass`; pipelines.rs: the pipeline with two R32Float/Rg16Float targets at the main
   sample count.
3. New `render/grass_resolve.rs` + `grass_resolve.wgsl` (from `study_resolve.wgsl`
   `cs_resolve4` + `study_grass.wgsl` `grass_surface`, calling the real `shade_grass`),
   dispatched after the main pass's MSAA resolve.
4. Quality: High/Low 4 samples, Medium 2 (if supported), an expert toggle for 1× + TAA.
5. Verify with `grass_pass_times`, the golden shots vs meadow-look-1, and the editor's Game
   view numbers as in meadow-perf.md.

## 5. Honest caveats (what the prototypes simplify)

- **Lighting is a stand-in** (`study_common.wgsl`) at about 57–62 % of the live lit pass's
  cost; the projections scale for it, but the live gain must be measured after integration.
  Images are brighter than the engine's (mean luma of the lower half 152 vs 116,
  `study_live_vs_study_{wide,close}.png`) and have no fog haze, sky, mountains, trees, clouds
  or water. All look comparisons are variant vs the study's own A, not vs the engine.
- AO and froxel fog are constant textures (their fetches are paid, their producers are not);
  the sun cascades are rendered once per camera and not timed (the live shadow passes are
  untouched by every variant anyway).
- **Wind frozen** during timing (as in `grass_pass_times`): neither the live nor the study
  evaluates last frame's blade. With wind the forward path adds a second blade evaluation per
  vertex; the resolve would add one per shade (fewer).
- One field per camera position: the motion runs keep the start position's chunks and
  prefixes for their 1.5 s walk.
- The study TAA is simple (bilinear history, variance clip); the engine's may treat
  single-sample shimmer somewhat better or worse. The shimmer figures are relative.
- E's parallax layer is not art-tuned; C's coverage model is crude (the timing conclusion
  does not depend on either). D is one parameterisation of "clumps" (our cards already are
  4-leaf clumps); bespoke 20–50-blade clump textures would still be fragment-bound.
- The depth and visibility pass counters are before the depth test (upper bounds); the lit
  pass counts are exact (forced early depth).
- Other agents compiled and ran GPU timings during the study; every number is the fastest of
  3–4 interleaved rounds, and runs still differ by ±0.4 ms.
- The behind shot is the wide camera turned 180°, not the tree study's exact camera.

## 6. Files

- Code: `crates/misty_engine/src/render/grass_study/` (`mod.rs`, `field.rs`, `gpu.rs`,
  `tests.rs`, `shaders/study_*.wgsl`); hook: `#[cfg(test)] mod grass_study;` in render/mod.rs.
- Runs (release, one at a time; `MISTY_STUDY=A,B4,...` picks variants, `MISTY_QUALITY`,
  `MISTY_GFX_SHOT=close|behind|all`, `MISTY_ROUNDS`):
  - `cargo test --release -p misty_engine --lib study_times -- --ignored --nocapture --test-threads=1`
  - `... study_calc`, `... study_shots`, `... study_motion` (add `Gc` to `MISTY_STUDY` for
    G with fast clouds), `... study_vs_reference`, `... study_live_vs_study`
  - `study_shaders_validate` (not ignored): every study shader parses and validates.
- Screenshots (`docs/screenshots/gfx/`):
  - `study_A_{wide,close,behind}.png`: the study baseline.
  - `study_<variant>_<shot>.png`: variant | A | difference ×8, a full-resolution 640 × 360 crop
    on top and the whole frames at a third below (all variants wide; A1, B, B2, B4, B4H, D, E,
    F, G, H also close and behind).
  - `study_overview_{wide,close,behind}.png`: every variant at quarter size, in the order A,
    A1, B, B4, B2, C, D, E, F, G, H, H2, BE, B4H, BEG, AE, AEG (row by row, 4 per row).
  - `study_motion_<variant>.png`: the last frame of the walk-and-turn, variant | A | ×8.
  - `study_vs_reference.png`: the reference photo | A | B4 (wide).
  - `study_live_vs_study_{wide,close}.png`: the live renderer (High) | the study's A.
