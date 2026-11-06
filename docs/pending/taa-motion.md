# Motion vectors, TAA quality and cloud stability (taa-motion)

Complaints: "TAA is making it smoother yes but more like blurry, it's causing slight shaking,
and a lot of motion blur that hurts the eyes" and "when looking around, the clouds are shaking
even when TAA or FXAA are disabled".

## Decisions

- **Motion vectors (velocity buffer).** The main pass has a third colour target, `Rg16Float`
  MSAA (no resolve; TAA reads sample 0 like it reads the depth), holding last frame's uv minus
  this frame's per pixel (`velocity_of` in common.wgsl). The screen-space prepass (AO,
  contact shadows, shadow mask, SSGI / SSR) writes the same into a single-sample target, since
  those passes run before the main pass. Every vertex stage passes `prev_clip(..)` (xyw of last
  frame's clip position) as one `vec3` varying; the fragment stage turns it into the vector
  against its own raster position (the jitter taken out through `view.motion.xy`).
  - Static geometry: `prev_view_proj` × the same world position (terrain, water, sky — the sky
    reprojects its direction with w = 0, so only rotation moves it).
  - Moving objects: `Draw.prev_model` (the `draws[]` entry grew from 256 to 320 bytes). The
    previous matrix comes from `render/motion.rs`, which keeps last frame's draw list per
    target and pairs draws by index when the mesh is the same and the transform moved less
    than 3 m (a changed list or a teleport falls back to "still"). `MeshDraw` kept its shape
    (58 literals across the crates), so no caller changes.
  - Grass: `grass_blade_at(vi, b, time)` evaluates the blade a frame earlier
    (`grass.origin.w - grass.screen.w`, dt from the per-target grass clock), so gusts and
    flutter get real vectors. The vertex stage runs the blade twice in the main pass and the
    prepass while TAA is on (shadow casters don't need it; without TAA dt is 0 and the
    second evaluation is skipped).
  - Skinned meshes fall back to object motion (the posed vertex buffer is not kept).
  - Fade / Transparent surfaces and particles leave the vectors of what is behind them (they
    don't write depth either); the pipeline masks the target.
- **Consumers.** AO resolve, the temporal shadow mask, the SSGI / SSR temporal passes and the
  colour-history fetches of SSGI / SSR hits now reproject through the prepass velocity
  (`reproject_px`); their depth-based disocclusion tests stay. The froxel fog and sharp shafts
  keep their matrix reprojection (volumes, not surfaces). The clouds keep theirs too (the sky:
  rotation only, exact).
- **TAA** (`taa.wgsl`): nearest-depth 3×3 velocity dilation (thin foreground edges move
  with the foreground); five-tap Catmull-Rom history fetch (bilinear would soften a little
  more every frame); variance clipping in YCoCg of the tonemapped colour, γ 1.25 at rest →
  1.0 in motion (anti-flicker without ghosting); depth rejection (the history stores view
  depth in alpha, compared with the pixel's depth in the previous view, 10 % tolerance);
  blend = Quality ▸ TAA Blend (default 0.1) rising to 0.3 above 12 px/frame of motion, 1.0 on
  disocclusion. A separate pass (`fs_taa_sharpen`) writes the history to the image through a
  neighbourhood-clamped unsharp mask (Quality ▸ TAA Sharpness, default 0.4): the image is
  crisp, the history itself never gets sharpened, so nothing builds up frame on frame.
  Jitter: Halton(2, 3), 8 phases. With TAA off the view matrix is the camera's, bit for bit
  (`jittered` returns it unchanged for a zero offset, `view.motion.xy` is 0, so every pass
  that reads the view uniform sees no jitter; `taa_off_draws_with_the_unjittered_camera`).
- **Clouds and froxel fog under TAA**: both are traced / lit without the jitter, so their
  lookups in the forward shaders take the jitter back out (`- view.motion.xy`); otherwise
  they swung by the jitter every frame.
- **No motion-blur post effect exists**; nothing was added.

## Cloud shaking: root cause

**Imprecise view rays, not the cloud history.** Every pass that needed the direction of a
pixel (the cloud raymarch and its temporal reprojection, the sky shader — sun disc, sky,
HDRI, and through `vfog_sky` the clouds' screen lookup — and the froxel fog) built it by
unprojecting the near and far planes through the inverse view-projection in f32. The far
plane's homogeneous w is `1/n − (f − n)/(f·n)`: catastrophic cancellation that grows with
the near / far ratio, and the eye translation inside the inverted matrix turns that error
into a sideways error. The resulting ray is off by an amount that changes with every camera
orientation, so as the view turns the clouds jump around their true position by up to
several pixels — with TAA on or off. The Scene view's dynamic clipping makes it worst: close
to the ground near = 1 cm, far = 1 km, and a big terrain puts the eye hundreds of metres from
the origin. Measured (f32 model of the shader maths, 1080p, 60° fov):

| camera | worst ray error |
|---|---|
| near 0.3 / far 1000, eye near the origin (headless tests, Game view defaults) | < 0.01 px |
| near 0.034 / far 17 000, eye 300 m out (Scene view, distance 17) | 0.9 px |
| near 0.01 / far 1000, eye 950 m out (Scene view flying low over a big terrain) | 7.9 px |

That is why none of the earlier turning tests (near 0.1–0.3, eye at the origin) saw it, and
why the clouds measured stable there: their temporal resolve, blue-noise offsets and
reprojection were fine (raw trace vs converged: 0.7 levels; sky vs its exact reprojection
while turning at 1–60 px/frame: 0.1–0.5 levels, edges included).

**Fix:** exact rays everywhere the direction matters.
- The view uniform carries the camera rotation (quaternion) and the half-fov tangents
  (`view_rotation`, `view_tan`, the last 32 bytes of the 512-byte view slot);
  `view_ray(ndc)` in common.wgsl rotates `((ndc − jitter) · tan, 1)`. The sky uses it.
- The cloud uniform's matrix is the inverse view-projection of the camera moved to the
  origin (`inv_rays`), unprojected at the near plane (`view_dir` in cloud_common.wgsl).
- The froxel uniform's inverse view-projection drops the eye translation too (eye added
  back in `froxel_world`).
- `clouds_hold_still_while_looking_around` (Scene-view fly camera, irregular mouse-look
  steps, TAA off): sky off its exact reprojection by **2.94 levels, 12.3 % of pixels > 8
  levels** before the fix, **0.30 levels, 0 %** after (a still camera: 0.06).

(Earlier on this branch: clouds and froxel fog sampled at the unjittered screen position,
which removed their swing under TAA jitter.)

## Costs

(1080p, MeadowDemo, High, RTX 4060, median GPU time of 100 frames; `taa_cost` on the
baseline build 40a612e and on this branch)

Four interleaved rounds (baseline, this branch, and two diagnostic builds of the branch); the
whole-frame time drifts by ±2 ms between runs on this machine, so the numbers are paired
differences per round, then averaged:

| | TAA off | TAA on |
|---|---|---|
| baseline 40a612e (absolute, range) | 31.0–34.4 ms | 31.4–35.1 ms |
| this branch − baseline | **+2.2 ms** (+1.5…+2.7) | **+2.5 ms** (+2.1…+2.8) |
| branch with the velocity writes masked off − baseline | ≈ 0 (−0.7…+1.0) | +0.6 (−0.3…+1.9) |
| branch without the grass's second (previous-frame) blade evaluation | no change (noise) | no change (noise) |
| TAA resolve + sharpen pass (on − off) | — | baseline 0.8 ms, branch 1.1 ms |

So the motion vectors cost about **2 ms at 1080p in MeadowDemo**, almost all of it the extra
4× MSAA `Rg16Float` colour target written by every main-pass fragment (dense grass with
alpha-to-coverage is the worst case for that: heavy overdraw on four samples). Evaluating
the blade a frame earlier is nearly free (the compiler shares everything but the wind), and
with TAA off it is skipped anyway (`grass.screen.w` = 0). The TAA pass itself got ~0.3 ms
dearer (Catmull-Rom, variance clip, the sharpen pass).

## Results

Headless GPU tests (`render::taa_gpu_tests`, `render::clouds::gpu_tests`), baseline = the same
tests on 40a612e:

| test | baseline | this branch | limit |
|---|---|---|---|
| still scene: edge contrast TAA on / off after 16 frames | 91.2 % | **98.6 %** | ≥ 90 % |
| turning 1 px/frame: shifted frame difference, TAA off / on (still TAA) | 0.90 / 1.17 (0.68) | 0.90 / **1.18** (0.63) | on < off + 1.5 |
| moving cube (10 px/frame): ground behind it vs a clean frame, 2 frames later | 0.06 levels | 0.08 levels | < 4 |
| grass in the wind (0.12): pixels changing > 8 levels a frame, TAA off → on | 5.21 % → 0.98 % | 5.22 % → **1.80 %** | on < ½ off |
| grass in the wind: edge contrast of the blowing grass vs stopped, TAA on | — | **92.2 %** | > 85 % |
| TAA off: view-projection bit-exact the camera's (4 frames) | — | yes | exact |
| looking around, Scene-view camera, TAA off: sky vs exact reprojection (worst pair) | 2.94 levels, 12.3 % > 8 | **0.30 levels, 0 %** | < still + 0.6, < 0.5 % |
| turning 1 px/frame, clouds, TAA off (shifted / still) | — | 0.22 / 0.08 | < still + 2 |

The baseline's low 0.98 % in the wind is its smear: blades blended over ~8 frames hardly
change per frame because they are a blur (wind crop edge contrast 16.4 vs 24.2 now, see
below). Now TAA still removes two thirds of the shimmer of TAA off, and the moving blades
keep 92 % of the contrast of the same blades with the wind stopped.

## Screenshots

`docs/screenshots/gfx/taa_{before,after}_{static,turning,wind}.png` — 2× crops of the middle
of MeadowDemo (TAA on, High): still after 28 frames, turning 3 px a frame, crouching in the
grass in the wind. Rendered by `taa_screenshots` on the baseline (`MISTY_TAA_TAG=before`) and
this branch (`after`). Edge contrast (mean luma gradient) of the crops, before → after:
static 9.6 → 12.9 (+35 %), turning 9.4 → 10.2 (+8 %), wind 16.4 → 24.2 (+48 %).

## What's left

- Velocity cost (~2 ms at 1080p in grass): the main pass writes its own 4× MSAA vectors even
  when the screen-space prepass (full resolution, already writing vectors) runs. Taking
  TAA's vectors from the prepass when it runs, and dropping the main-pass target, would save
  most of it; it needs every main-pass pipeline rebuilt without the third target.
- Skinned meshes reproject with their object motion only (the posed vertices of last frame
  are not kept); fast limb motion can still ghost a little under TAA.
- Fade / Transparent surfaces and particles write no motion vectors (they take the vectors of
  what is behind them).
- The editor grid still unprojects near / far planes (only the grid shimmers there).
- The "before" screenshots, baseline test numbers and baseline costs were taken on a
  separate checkout of 40a612e (with the same test file); the cloud "before" number by
  reverting the ray fix on this branch for one run.
