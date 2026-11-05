# gfx-shadows: area-light-correct soft shadows (pending merge)

Goal: Cycles-like soft shadows (clean, wide, smooth penumbrae that grow with the distance
from the occluder, no banding / grain / blockiness) from rasterised shadow maps only — no
ray tracing.

## Decisions

1. **Project the light's real shape through the receiver (local lights).** PCSS kernels are
   no longer a circle of "source radius" in shadow-map uv. For a receiver p, a source point at
   shape coordinates s lines up with the blocker texel at uv(p + s · axes · (d − d_b) / d_b);
   the shader linearises uv around p (two extra projections per pixel) and gets a 2×2 kernel
   matrix. Spot / point lights use a disk of their radius facing the receiver (elliptical in
   faces seen obliquely), rectangle lights their own half axes, sampled over the whole square
   (Vogel disk → square by the area-preserving inverse concentric map). *Why:* a 1.6 × 0.4 m
   panel now softens shadows 4× more across its long side, like Cycles; the old code used
   half the mean side as a disk radius.
2. **Rectangle lights get a hemicube** (5 faces, 6 when two-sided, 92° each, in the
   rectangle's own frame) when soft shadows are on; with Soft Shadows Off they keep the
   single 120° map (old cost). Forward face is layer 0 so the fog, which reads one layer per
   light, still sees the faces under the light without touching volumetrics. *Why:* the 120°
   map left floors more than 60° off-axis unshadowed — most of a room.
3. **Temporal shadow mask instead of more taps** (render/shadow_denoise). A compute pass over
   the existing screen-space depth + normal prepass evaluates exactly the lit shaders' shadow
   functions (shadows.wgsl is appended to the compute shader with a small prelude) for up to
   four soft-shadowed lights (sun first), with interleaved-gradient-noise rotation offset per
   frame (Jimenez 2014) and per-frame radial jitter, so 16–32 taps × ~16 frames average
   hundreds of samples. History is reprojected through the camera (bilinear over the 4
   nearest texels whose depth matches within 3 %), clamped to this frame's 3×3 neighbourhood
   (workgroup shared memory, one pass) so moving casters don't ghost, newest frame weight
   ≥ 1/16. *Why not a per-light texture in the forward pass:* no TAA exists to accumulate; a
   screen-space mask is the cheapest place to keep history, and lit shaders fall back to the
   direct path wherever the mask doesn't match (transparent surfaces, MSAA edges, other
   lights) via a depth check.
4. **Penumbra-sized denoise.** The trace records each light's filter radius at the receiver;
   the blur (12 Vogel taps, depth-plane + normal weights) is at most half the penumbra in
   pixels (wider while history is young), so contact-hardened edges stay crisp and wide
   penumbrae get smooth. Output ping-pongs into the buffer the next trace overwrites (two
   16-byte-per-pixel buffers per view, 33 MB at 1080p).
5. **Quality.** Levels map to taps: Low = hard compare (unchanged path, no mask, no prepass,
   one rect map), Medium = 8/12 taps direct, High = 12/20 + temporal + denoise, Ultra = 16/32
   + temporal + denoise. Widest filter 48 / 64 / 96 texels per tier. New toggles
   `shadow_temporal` / `shadow_denoise` (Project Settings ▸ Quality, shown when soft shadows
   are on). The mask turns on the screen-space prepass and turns GPU culling off (as AO does).
6. **Per-light Shadow Softness** (Light ▸ Shadows, multiplier, serde default 1, left out of
   files at 1). Rectangles have no radius field: softness scales their own axes.
7. **Bias tuned to the filter radius**: compares get 0.02 m of extra depth bias per metre of
   sample offset from the receiver (on top of the receiver-plane slope), removing acne on
   curved receivers under wide kernels without adding peter-panning at contact (offset ≈ 0
   there).
8. **Contact shadows harden at contact**: the screen-space ray's occlusion now falls off as
   (1 − t)² with the hit distance, a crisp dark line at the gap that fades into the PCSS
   penumbra.

## Progress

- [x] light-shape PCSS kernels (disk / rectangle, anisotropic), softness multiplier
- [x] rectangle hemicube shadows
- [x] temporal mask + penumbra-sized denoise, quality toggles, editor UI
- [x] contact-hardening contact shadows, radius-scaled bias
- [x] SoftShadowsDemo generator + test
- [x] GPU tests: penumbra vs blocker height / light size, rect anisotropy + wide angle,
      temporal smoothness and 1-px stability, Low path unchanged

## Measurements (RTX dev machine, SoftShadowsDemo 1920×1080, GPU timestamps, median of 40)

| setup | GPU ms |
|---|---|
| Low (hard) | 2.17 |
| Medium (direct 8/12) | 2.81 |
| High taps without mask (12/20) | 3.19 |
| High (temporal + denoise) | 3.97 |
| Ultra taps without mask (16/32) | 3.68 |
| Ultra (temporal + denoise) | 4.73 |

The mask (prepass + trace + blur, two lights) costs ~0.8–1.05 ms at 1080p.
Tests (headless, 64 px/m): spot r = 0.3 m penumbra 9 px at h = 1 → 43 px at h = 2;
h = 1.5: r = 0.15 → 9 px, r = 0.45 → 28 px; 1.6 × 0.4 m rectangle: 51 px across its width
vs 17 px across its depth; penumbra roughness 0.0178 direct → 0.0027 temporal; 1-px camera
move after convergence: mean difference 0.22 / 255, worst 19.

Screenshots: docs/screenshots/gfx/shadows_before.png (main), shadows_after.png (High),
shadows_after_medium.png, shadows_penumbra_direct_vs_temporal.png (crop: Medium direct vs
High temporal), shadows_rect_anisotropic.png, shadows_spot_penumbra.png.

## Left / ideas

- The mask covers four lights; more soft lights use the direct path.
- Moving lights / casters converge again over ~16 frames (clamped, so no smearing, but a
  little more grain right after motion).
- Hard shadows of rect lights use the hemicube when soft shadows are on in the level.
- Like AO, the mask's prepass turns GPU culling off in views that use it (High / Ultra
  with a soft-shadowed light). Determinism tests of other features now render with the
  mask off (gpu_checks, screen_space effects-off).
- The editor never ran interactively in this branch; only headless renders were checked.
