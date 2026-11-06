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
    prepass (shadow casters don't need it).
  - Skinned meshes fall back to object motion (the posed vertex buffer is not kept).
  - Fade / Transparent surfaces and particles leave the vectors of what is behind them (they
    don't write depth either); the pipeline masks the target.
- **Consumers.** AO resolve, the temporal shadow mask, the SSGI / SSR temporal passes and the
  colour-history fetches of SSGI / SSR hits now reproject through the prepass velocity
  (`reproject_px`); their depth-based disocclusion tests stay. The froxel fog and sharp shafts
  keep their matrix reprojection (volumes, not surfaces). The clouds keep theirs too (the sky:
  rotation only, exact).
- **TAA** (`taa.wgsl`): nearest-depth 3×3 velocity dilation; Catmull-Rom history fetch
  sharpened by an unsharp mask against its own 4-tap bilinear blur (Quality ▸ TAA Sharpness,
  default 0.4, applied before the clip so ringing is bounded); variance clipping in YCoCg of
  the tonemapped colour, γ 1.25 at rest → 1.0 in motion (anti-flicker without ghosting);
  depth rejection (history stores view depth in alpha, compared with the pixel's depth in the
  previous view, 10 % tolerance); blend = Quality ▸ TAA Blend (default 0.1) rising to 0.3
  above 12 px/frame of motion, 1.0 on disocclusion. Jitter: Halton(2, 3), 8 phases. With TAA
  off the view matrix is the camera's, bit for bit (`jittered` returns it unchanged for a zero
  offset; `taa_off_draws_with_the_unjittered_camera` checks it).
- **No motion-blur post effect exists**; nothing was added.

## Cloud shaking: root cause

(filled in below once measured)

## Costs

(1080p, RTX 4060; measured with `taa_cost`, baseline = main before this branch)

## Results

(test numbers)

## Screenshots

`docs/screenshots/gfx/taa_{before,after}_{static,turning,wind}.png` — 2× crops of the middle
of MeadowDemo (TAA on, High): still after 24 frames, turning 3 px a frame, crouching in the
grass in the wind. Rendered by `taa_screenshots` on the baseline (`MISTY_TAA_TAG=before`) and
this branch (`after`).

## What's left

