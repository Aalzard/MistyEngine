# Temporal upscaler (upscaler)

Goal: MeadowDemo ran at 43 / 57 / 92 FPS (High / Medium / Low) at native 1920×1080 on an RTX
4060, and the target is 100+. Shipped games usually render at 50–77 % of the output
resolution and rebuild a sharp image temporally. This branch adds a temporal upscaler: the
scene renders at a lower internal resolution, and the TAA resolve rebuilds the full output
resolution.

## What it is

**Quality ▸ Upscaling: Off / Quality (67 %) / Balanced (58 %) / Performance (50 %) / Custom**
(per side; Custom = Quality ▸ Upscaling Scale, 33–100 %). The setting is in Project
Settings ▸ Quality and in the Scene view's Quality dropdown, and it applies to the Scene
view, the Game view and the player. Turning it on also turns temporal anti-aliasing on (the
upscaler is TAA at a lower resolution, so it runs whatever the TAA tick box says).
Quality ▸ Upscaler picks the backend. Only *Misty Temporal* exists; FSR / DLSS / XeSS would
be further `UpscalerBackend` variants consuming the same inputs (see below). No SDK was added.

The existing **Render Scale** (a stretch of the whole frame, post effects included, to the
window) is unchanged and still multiplies on top of this. It is kept for pixel-art looks and
for very weak GPUs.

### Pipeline

- **Render target** (`render/target.rs`): a view target has an *internal* size
  (`RenderTarget::size`, which every scene pass already used) and an *output* size
  (`output_size`). The internal size covers the MSAA colour, depth, motion vectors, glow, the
  resolved HDR image and every screen-space buffer (AO, contact shadows, SSGI / SSR, the
  shadow mask, froxel fog, clouds, GPU culling's Hi-Z). The output size covers the upscaled
  HDR image, the bloom chain, the tonemapped image, FXAA / sharpen and the canvas UI.
  `Renderer::create_view_target` / `view_target_fits` make and check targets for the active
  level; `create_target` stays native, so thumbnails, previews and probe captures (one-off
  renders without history) are never upscaled.
- **Jitter** (`taa.rs`): Halton (2, 3) in *internal* pixels, cycling through
  8 / scale² positions (8 native, 18 Quality, 24 Balanced, 32 Performance). Each internal
  pixel covers 1 / scale² output pixels, and each of those needs samples near its centre
  (`upscaling_cycles_through_more_jitter_positions`).
- **Resolve** (`fs_taau` in `taa.wgsl`): runs per output pixel with the history at the output
  resolution.
  - Reconstruction: the 3×3 internal samples nearest the output pixel, each placed where it
    actually saw the scene (pixel centre − jitter). A Gaussian kernel in *output* pixels,
    `exp(−4.6 d²)` (σ ≈ 0.33 px, about the footprint a native pixel's jittered samples
    cover), gives this frame's estimate. The nearest sample's kernel weight decides how
    much of the frame goes in: a sample right on the pixel centre counts a lot, one an
    internal pixel away hardly at all. So the history converges to the full resolution, not
    to a stretched low-resolution image.
  - Blend share = TAA Blend × that weight × √(output pixels per internal pixel), capped at
    0.85, rising with motion like native TAA.
  - The neighbourhood variance clip (YCoCg, γ 1.25 still → 1.0 moving), the nearest-depth
    velocity dilation, the Catmull-Rom history fetch and the depth-based disocclusion test
    are the native TAA's, fed by the internal-resolution colour, depth and motion vectors.
  - Reactive / disocclusion: where the history can't be used (first frame, a camera cut,
    off screen, or depth rejection: the history saw a surface at another depth than this
    one had last frame), the pixel falls back to a smooth spatial upsample of this frame
    (σ ≈ 0.43 internal px) instead of a blocky or ghosted value.
- **Sharpening**: the existing neighbourhood-clamped unsharp pass writes the output image
  from the history (never fed back). It is stronger the lower the scale: TAA Sharpness ×
  1.0 native, rising to × 1.6 at 67 % and below (`sharpen_gain`).
- **Mip bias**: material textures (mesh, cutout depth / AO prepasses, trees, terrain layers)
  sample with `log2(render scale)` bias (−0.58 Quality, −1 Performance) through
  `view_tan.z` of the view uniform. They pick the mip the output resolution would have
  picked, and the temporal resolve averages the extra aliasing away. It is 0 at native
  resolution (bit-identical to before) and 0 in shadow passes (their view slots carry 0).
  Not biased: custom `.shader` materials, water normal maps, particles, UI.
- **Post at output resolution**: bloom, grading / tonemapping, vignette, FXAA / sharpen and
  the canvas UI (Camera Far Plane videos look the internal depth up by screen position).
  Gizmo lines and the selection outline are drawn in the main pass, so they are upscaled
  too. They stay readable, slightly softer at 50 %.
- **No temporal resolve this frame** (wireframe draw mode, a Frame Debugger replay that stops
  before it): the internal image is stretched bilinearly to the output (`fs_taa_stretch`).

### Thin features (grass)

- Grass blades thinner than ~0.8 px are widened to that width by the grass shader
  (`grass_screen` passes the pixel size). With upscaling this is the *internal* pixel. I
  tried sizing them by the output pixel instead (thinner, more sub-pixel blades for the
  upscaler to resolve). It measured hazier, not sharper (wide-shot crops), so it stays at
  the internal pixel.
- The first version gave this frame's sample full weight × (output px per internal px), so
  the average share over a jitter cycle matched native TAA. That made thin grass in the
  wind flicker in **2.3 %** of the pixels at Performance (native without TAA: 0.2 %), because
  single well-placed samples moved a pixel by 40 %. The √ gain brings it to native's level:

| gain exponent | Quality flicker | Performance flicker | Quality edge contrast vs still |
|---|---|---|---|
| 1 (first version) | 0.47 % | 2.27 % | 96.9 % |
| **0.5 (shipped)** | 0.18 % | 0.39 % | 96.4 % |
| 0 | 0.11 % | 0.14 % | 95.7 % |

(MeadowDemo crouching in the grass, wind 0.12, 640×360, pixels changing by more than 8
levels between frames 40 and 41; native without TAA in the same runs: 0.20–0.24 %. Then the
kernel was narrowed from Blackman-Harris' σ 0.47 to σ 0.33 with the sharpening gain, which
measured sharper and lowered flicker further: Quality 0.14–0.17 %, Performance 0.23–0.29 %.)

## Costs (MeadowDemo, RTX 4060)

`upscale_cost`: the saved player view, median GPU time of 80 frames per configuration, the
fastest of 3 interleaved rounds. Other sessions' GPU tests shared the card during some
rounds; those show up as outliers, e.g. 36.2 vs 17.2 ms, and are dropped by taking the
fastest. Rounds otherwise agreed within 0.2 ms. FPS = 1000 / GPU ms (the GPU-bound rate;
the CPU side of a render is 1.5–5 ms). The levels are measured at their own settings
**without** their Render Scale (the Low level's 0.75 stretch would mix two resolutions).
Low has no TAA, so its native row has no resolve; its upscaled rows do.

### 1920×1080

| level | Off (native) | Quality 67 % | Balanced 58 % | Performance 50 % |
|---|---|---|---|---|
| High | 22.73 ms · **44 FPS** | 16.05 ms · **62 FPS** | 14.57 ms · **69 FPS** | 13.34 ms · **75 FPS** |
| Medium | 17.16 ms · **58 FPS** | 11.33 ms · **88 FPS** | 10.63 ms · **94 FPS** | 9.43 ms · **106 FPS** |
| Low | 10.47 ms · **96 FPS** | 7.38 ms · **135 FPS** | 6.55 ms · **153 FPS** | 5.80 ms · **172 FPS** |

### 2560×1440

| level | Off (native) | Quality 67 % | Balanced 58 % | Performance 50 % |
|---|---|---|---|---|
| High | 31.44 ms · **32 FPS** | 20.87 ms · **48 FPS** | 18.41 ms · **54 FPS** | 16.40 ms · **61 FPS** |
| Medium | 24.58 ms · **41 FPS** | 15.79 ms · **63 FPS** | 13.90 ms · **72 FPS** | 12.33 ms · **81 FPS** |
| Low | 15.19 ms · **66 FPS** | 10.22 ms · **98 FPS** | 8.97 ms · **112 FPS** | 7.85 ms · **127 FPS** |

The temporal resolve costs 0.42–0.48 ms at 1080p and 0.76–0.86 ms at 1440p, the same as
native TAA, since it runs per output pixel. Post-processing at the output resolution costs
0.14–0.28 ms. So 100+ FPS at 1080p: **Medium + Performance (106)** and **Low + any mode**.
**High does not reach 100 with upscaling alone** (75 at Performance).

### Where High's time goes (1080p, pass timer)

| part | native | Quality | Performance |
|---|---|---|---|
| Shadow maps (cascades, local) | 1.28 | 1.28 | 1.28 |
| Grass near shadow map | 1.49 | 1.48 | 1.48 |
| AO prepass + GTAO, shadow mask | 2.47 | 1.51 | 1.18 |
| Main: terrain | 2.60 | 1.41 | 0.95 |
| Main: grass depth | 1.83 | 1.44 | 1.30 |
| **Main: grass lit** | **9.71** | **6.88** | **5.47** |
| Clouds, sun shafts, sky, rest | 2.27 | 1.09 | 0.71 |
| TAA / upscale resolve | 0.48 | 0.43 | 0.42 |
| Bloom, tonemap, finish | 0.16 | 0.14 | 0.14 |
| **frame** | **22.9** | **16.1** | **13.3** |

The pixel-bound passes scale with the pixel count (terrain −63 %, AO −52 %, sun shafts
−73 % at Performance). The grass doesn't: 1.2 M blades of vertex work stay
(grass lit −44 %, grass depth −29 %), and the two shadow maps (2.8 ms) don't change. Getting
High past 100 needs the grass's vertex cost down as well (the meadow-perf2 work); the
upscaler then multiplies whatever that saves in the pixel-bound part.

## Image quality

### Still: native vs upscaled (1920×1080, High, converged)

`docs/screenshots/gfx/upscale_<shot>_<mode>.png`: 1:1 crops (640×360 from the middle) for
shots wide (the player view), close (turned to the broad oak) and behind (turned round).
Modes are `native` (TAA), `quality`, `balanced`, `performance`, and `stretch50` (native TAA
at 960×540 stretched bilinearly: what Render Scale 0.5 gives). `_diff` images are
|mode − native| × 4. `upscale_<shot>_zoom.png` puts all five side by side at 2×.

| shot | mode | PSNR vs native | mean difference | edge contrast vs native |
|---|---|---|---|---|
| wide | Quality | 28.8 dB | 6.1 levels | 76 % |
| wide | Balanced | 28.3 dB | 6.5 | 70 % |
| wide | Performance | 27.6 dB | 7.2 | 65 % |
| wide | *stretch 50 %* | 27.3 dB | 7.6 | *38 %* |
| close | Quality | 32.9 dB | 2.7 | 79 % |
| close | Performance | 30.9 dB | 3.6 | 68 % |
| close | *stretch 50 %* | 30.2 dB | 4.0 | *40 %* |
| behind | Quality | 29.7 dB | 4.1 | 78 % |
| behind | Performance | 27.9 dB | 5.7 | 67 % |
| behind | *stretch 50 %* | 27.6 dB | 6.0 | *40 %* |

Honest reading:
- The upscaler keeps 65–79 % of native's edge contrast where a stretch of the same pixels
  keeps 38–40 %. Trees, branches, mountains and the horizon are close to native even at
  Performance (`upscale_close_zoom.png`).
- **Grass is where it shows.** The far meadow's blade texture is visibly softer than native
  at every mode, most at Performance (`upscale_wide_zoom.png`). Native has more real blade
  samples per pixel, and no reconstruction gets those back.
- PSNR barely separates the upscaler from the stretch (+0.3–2.7 dB). The meadow is
  per-pixel noise-like detail that neither reproduces exactly. Edge contrast and the
  zoomed crops show the difference better.
- Native TAA is itself 66 % of native-without-TAA's edge contrast in grass. That is the
  aliasing TAA removes, not detail.

### Headless checks (`render::upscale_gpu_tests`, run with the other GPU tests)

| test | result | limit |
|---|---|---|
| upscaled target: internal size, jitter in internal px (largest of 12 frames) | 160×100 → 320×200, ≤ 0.5 internal px, > 0.3 seen | exact |
| still test scene, edge contrast vs native TAA (stretch of the same pixels) | Quality 88 % (83 %), Balanced 85–87 % (79 %), Performance 81–84 % (75 %) | sharper and closer than the stretch; Quality > 85 % |
| turning 1 px/frame, shifted difference (native TAA off: 0.90) | Quality 0.95–1.07, Performance 0.78–1.18 | < off + 1.5 |
| moving cube at 10 px/frame, Performance: ground behind it vs converged | 0.20 levels | < 4 |
| grass in the wind (crouching), pixels flickering > 8 levels / frame (native TAA off) | Quality 0.14–0.18 %, Performance 0.23–0.39 % (0.19–0.31 %) | < max(½ off, 1 %) |
| grass in the wind, edge contrast blowing vs stopped | Quality 95–96 %, Performance 96–97 % | > 85 % |

(Ranges: repeated runs. MeadowDemo renders aren't bit-identical from run to run, and native
TAA-off's own flicker varied 0.19–0.44 %.)

### Shimmer and smear while moving (`upscale_motion`, 960×540, High)

Same measures as the TAA GPU tests:
- **wind %**: still camera, wind blowing; pixels jumping > 8 levels between two frames.
- **turn diff**: turning 2 px/frame; consecutive frames compared shifted by the turn
  (lower = steadier).
- **smear**: edge contrast turning or walking forward at 1.4 m/s, as a share of the still
  image's.

| mode | wind % | still contrast | turn diff | turn smear | walk smear |
|---|---|---|---|---|---|
| native, TAA off | 0.21 % | 17.90 | 3.77 | 99 % | 100 % |
| native TAA | 0.46 % | 11.89 | 1.23 | 72 % | 75 % |
| Quality | 0.17 % | 9.07 | 0.81 | 74 % | 79 % |
| Balanced | 0.23 % | 8.24 | 0.79 | 75 % | 80 % |
| Performance | 0.15 % | 7.57 | 0.64 | 74 % | 79 % |
| 50 % stretched (TAA) | 0.16 % | 4.68 | 0.85 | 84 % | 88 % |

- Upscaled grass shimmers no more than native TAA (less, in fact) and is steadier while
  turning.
- In motion, upscaled modes lose about the same share of their still contrast as native
  TAA (74–80 % vs 72–75 %). No extra smear, but they start from a softer still image.
- The stretch "keeps" 84–88 % only because it is already blurry.

## Using it

- Project Settings ▸ Quality ▸ Upscaling (and Upscaling Scale for Custom, Upscaler). Or
  the Scene view's Quality dropdown ▸ Upscaling.
- Scripts and tools: `QualityLevel::upscaling`, `upscaling_scale`, `upscaler`,
  `upscale_ratio()`, `temporal()`. Renderer: `create_view_target`, `view_target_fits`,
  `RenderTarget::{output_size, render_scale, upscaled}` (`size()` is the internal size).
- Self-screenshots: `MISTY_UPSCALING=Off|Quality|Balanced|Performance` with
  `MISTY_SCREENSHOT` in the editor (previewed level) and in the player (every level).
  Checked: the editor's Scene view of MeadowDemo at Off, Quality and Performance, and the
  player at Off and Performance.
- The defaults are unchanged: every built-in level ships with Upscaling Off, and levels
  saved before this load as Off. Suggested (not applied): Medium → Performance or Balanced
  for 100+ at 1080p, High → Quality where 60 FPS is the goal.

## Tests and measurements

- `cargo test -p misty_engine upscale` runs the functional GPU checks. Unit tests:
  `render::taa::tests` (jitter phases and coverage, sharpen gain, shader validation),
  `render::target::tests`, `quality::tests::upscaling_modes_and_old_files`.
- Measurements, in release, one at a time (the GPU must be otherwise idle):
  `cargo test --release -p misty_engine upscale_cost -- --ignored --nocapture --test-threads=1`
  (`MISTY_SIZES`, `MISTY_QUALITY`, `MISTY_ROUNDS`, `MISTY_PARTS=1`), `upscale_screenshots`
  (`MISTY_UPSCALE_SHOTS`, `MISTY_UPSCALE_DIR`), `upscale_motion`.

## What's left

- **High at 100+** needs the grass vertex cost cut (meadow-perf2) on top of this. The two
  should compound: at Performance the grass is ~6.8 of 13.3 ms.
- Vendor backends (FSR 2/3, DLSS, XeSS) can plug in at `UpscalerBackend` with the inputs in
  `taa::Resolve`. They would also want a reactive mask (transparent / particle pixels),
  which the engine doesn't produce yet.
- Gizmos and the selection outline are drawn before the upscale (slightly soft at 50 %).
  Drawing them after it, at the output resolution, needs a depth at the output size.
- Custom `.shader` materials and water normals don't get the mip bias.
- The variance clip uses the internal 3×3 neighbourhood. A clip at the output resolution
  (or a luma-weighted one) might keep more grass contrast at Performance.
