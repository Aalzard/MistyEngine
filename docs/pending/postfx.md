# Post effects and decals (branch postfx, round 17)

Roadmap round 17 (17A post stack, 17B decals): **Depth of Field, Motion Blur, Auto Exposure**
as Post-process Volume overrides and a **Decal Projector** component, Unity HDRP / URP-like.
Every one is off by default, so existing scenes render as before (golden check unchanged).

## What was built

### Volume overrides (Post-process Volume ▸ Depth of Field / Motion Blur / Auto Exposure)
Scene data `scene/postfx_overrides.rs` (left out of scene files while untouched), resolved into
`PostSettings`, edited in the Inspector by `panels/inspector/post_volume_fx.rs` (card sections
with the usual override tick).

- **Depth of Field** (URP's modes):
  - *Gaussian*: Start, End, Max Radius (0.5–1.5; 1 = 10 px at 1080p), High Quality Sampling.
    Far field only, like URP.
  - *Bokeh*: Focus Distance, Focal Length (mm, on a 24 mm sensor), Aperture (f-stop), Blade
    Count, Blade Curvature, Blade Rotation, Quality (Low / Medium / High = 25 / 49 / 81 gather
    samples, largest radius 8 / 12 / 16 px at 1080p). Thin-lens circle of confusion
    `c = A·F·(d − P) / (d·(P − F))`, near and far fields.
- **Motion Blur**: Intensity (shutter share, 0.5 = 180°), Maximum Velocity (share of screen
  height), Sample Count (4–32), Camera Motion Blur (off = moving objects only), Camera Clamp
  (longest camera blur, its own limit). Per-object and camera motion from the existing motion
  vectors. The Scene view leaves motion blur out (as Unity does); the Game view shows it.
- **Auto Exposure** (eye adaptation): Metering Mode (Average, Spot, Center Weighted), Limit
  Min / Max and Compensation in EV, Adaptation (Fixed / Progressive) with Speed Dark to Light
  and Light to Dark, Histogram Low / High %. EV is relative to the engine's fixed exposure:
  EV 0 = as without Auto Exposure, a scene a stop brighter than mid grey gets EV 1. Off =
  fixed exposure exactly as before (the composite multiplies by 1).

### Decal Projector (GameObject ▸ Rendering ▸ Decal Projector)
`scene/decal.rs`: Size (width, height, projection depth), Pivot, Base Map (colour + opacity in
alpha), Normal Map, Mask Map (HDRP layout: R metallic, G occlusion, A smoothness), Color,
Tiling / Offset, Opacity, Affects Base Color / Normal / Mask, Normal Blend, Metallic,
Smoothness, Emission (+ intensity), Draw Distance, Start Fade, Angle Fade (0..180 = off, as in
Unity), Priority (higher on top; ties in Hierarchy order). Projects along local +Z; made
pointing down so it lands on the ground. Inspector (`decal_editors.rs`) with texture slots
(drag and drop), a box + arrow + "up" chevron gizmo (`decal_gizmos.rs`), Hierarchy icon,
picking icon, search menu entry. Works on meshes, virtual geometry and terrain.

Sample textures (procedural, CC0, made by `sample_postfx.rs`) in `SampleProject/Assets/Decals`:
Dirt, Moss (+ normal, mask), Cracks (+ normal), Puddle.

### Demo
`SampleProject/Assets/Scenes/PostFxDemo.scene` (generator `crates/misty_editor/src/
sample_postfx.rs`, `write_postfx_demo`; `postfx_demo_matches_its_generator`): a courtyard with
a brick wall, pillars and lamps into the distance (Bokeh focused on the wall, lamps for bokeh),
an orbiting ball and a spinning fan (motion blur in Play mode), an archway over the camera
(auto exposure), a small terrain hill, and 9 decals (dirt, moss on the ground, the wall foot
and the hill, cracks on the wall and the ground, a wet puddle with priority 2).

## Design choices

### Where the camera effects run
After the temporal resolve (TAA or the upscaler), at the output resolution, in HDRP's order:
exposure measurement, depth of field, motion blur, then bloom and the composite
(`render/postfx/mod.rs`).
- The resolved image is converged and jitter-free, so the blurs need no temporal pass of their
  own and don't flicker; with the upscaler they blur the sharp output image. Depth and motion
  vectors are the main pass's (internal resolution, multisampled, sample 0 read by uv), so
  the circle of confusion and the blur vectors are in output pixels at any render scale.
- Depth of field and motion blur write into per-view chain images; the composite reads the
  last one through a composite group made for it (`RenderTarget::composite_group_for`).
  While they run the main pass keeps its depth (`keeps_depth`).
- Auto exposure writes a multiplier into a per-target storage buffer (`RenderTarget::exposure`,
  composite binding 3) that the composite applies to scene + bloom before grading. It is 1
  (exactly) while off, and reset to 1 when Auto Exposure is switched off.

### Depth of Field
Half-resolution: prefilter (colour + signed CoC; the nearest near CoC of the 2×2 so
foreground edges keep their spill), then for Bokeh 8×8 near/far CoC tiles, a dilation of the
near CoC to neighbouring tiles, and a ring gather inside the aperture polygon: the far field
takes background samples whose blur reaches the pixel, limited to the pixel's own CoC (a sharp
foreground stays sharp over a blurred background), the near field takes every foreground
sample whose blur reaches the pixel, with an area-based coverage (scatter-as-gather). Gaussian
is a separable CoC-aware Gaussian. A full-resolution composite blends sharp → far by the
pixel's CoC and lays the near field over it.

### Motion Blur
16×16 tile maximum and 3×3 neighbourhood maximum of the blur vectors, then a gather along the
neighbourhood's largest blur with a McGuire-style reconstruction filter (depth-aware cone and
cylinder weights), interleaved-gradient-noise jitter. The camera's share of each pixel's motion
comes from reprojecting its depth with last frame's camera; it is clamped on its own (Camera
Clamp) or dropped (Camera Motion Blur off), the object part is the rest of the motion vector.

### Auto Exposure
A compute histogram (128 bins of log2 luminance, -12..+12, every other pixel, metering
weights) and a one-workgroup average of the bins between the low and high percentages, then a
frame-rate-independent ease (`1 − e^(−speed·dt)`) towards the target EV within the limits. A
camera cut or the first frame jumps.

### Decals: screen-space decals into a decal buffer, applied by the forward shaders
The engine is a forward(-plus) renderer with MSAA and no G-buffer. A classic deferred decal has
no G-buffer to write into, and blending decals onto the lit image would leave them lit wrong
(no normal, no smoothness change). So decals use Unity URP's DBuffer idea, fitted to what the
renderer already has (`render/decals/mod.rs`):
1. While a decal is in view the **screen-space prepass** runs (`render/screen_space.rs`: depth +
   world normals of opaque meshes, virtual geometry, terrain, near grass and trees; the same
   prepass AO, contact shadows, SSGI and SSR use).
2. A **compute pass** (`decals.wgsl`) lists the decals per 8×8 tile from their screen
   rectangles (as bits, so the order stays Priority then Hierarchy), projects each pixel's
   world position into the decal boxes, applies the angle and distance fades, samples a 512²
   texture array (mips, footprint-based level) and blends albedo, normal, mask and emission
   (premultiplied) into a **per-pixel decal buffer**: four words per pixel appended to the
   screen-space texel buffer after the AO texels. No new binding: the lit fragment stage is
   at WebGPU's limit of 8 storage buffers.
3. The lit shaders' `shade` calls `decal_surface` (screen_space.wgsl) first: where the
   fragment's depth matches the prepass (not a transparent surface or a blade in front) it
   blends the decal values into the surface **before lighting**, so decals are lit, shadowed,
   reflect, take probes and GI like the surface under them. One hook covers meshes, terrain,
   virtual geometry, `.shader` materials and Shader Graphs.

Clustered forward decals were the alternative: every lit pipeline would need the decal list and
an atlas binding (no room in the binding budget), and they loop per shaded MSAA fragment
including overdraw, where the screen-space pass touches each pixel once and the lit shaders read
four words. Cost of the choice: the prepass when nothing else needs it (0.09 ms here), and GPU
culling falls back to CPU culling while the prepass runs (as with AO).

## Costs (RTX 4060, Vulkan, 1920×1080, Quality High, PostFxDemo, median of 40 frames)

`cargo test --release -p misty_engine --lib postfx_cost -- --ignored --nocapture --test-threads=1`
(under the GPU timing lock). Effect = the pass timer's own passes; frame = whole-frame GPU time.

| Configuration | Frame GPU ms | Effect ms | Passes |
|---|---|---|---|
| all off | 2.01 | – | |
| DOF Gaussian | 2.24 | 0.270 | Depth of field 0.270 |
| DOF Gaussian, High Quality Sampling | 2.26 | 0.286 | Depth of field 0.286 |
| DOF Bokeh Low (25 samples) | 2.42 | 0.470 | Depth of field 0.470 |
| DOF Bokeh Medium (49) | 2.59 | 0.639 | Depth of field 0.639 |
| DOF Bokeh High (81) | 2.81 | 0.865 | Depth of field 0.865 |
| Motion Blur, 8 samples | 2.57 | 0.615 | Motion blur 0.615 |
| Motion Blur, 16 samples | 2.96 | 1.007 | Motion blur 1.007 |
| Motion Blur, 32 samples | 3.75 | 1.803 | Motion blur 1.803 |
| Auto Exposure | 1.99 | 0.029 | Auto exposure 0.029 |
| Decals ×10 | 2.23 | 0.238 | decal pass 0.150 + prepass 0.087 |
| Decals ×200 | 2.29 | 0.330 | decal pass 0.244 + prepass 0.086 |

200 decals cost under the 0.5 ms the roadmap asked for (decal pass plus the prepass they turn on).

## Shots
`docs/screenshots/gfx/postfx_*.png` (PostFxDemo, 1920×1080, High, converged; test
`postfx_screenshots`): `postfx_off` (every effect off, no decals), `postfx_decals`,
`postfx_dof_bokeh`, `postfx_dof_gaussian`, `postfx_motion_blur` (camera turning),
`postfx_all` (decals, Bokeh, motion blur, auto exposure).

## Checks
- Golden shots (`golden_shots`, under the GPU lock): 20 shots, all within the limits (19 at
  mean 0.00, one at 0.01, 0.00 % changed); no reference updated.
- `cargo fmt --all`, `cargo clippy --workspace --all-targets --release`: 0 warnings.
- `cargo test --workspace --release --no-fail-fast`: engine 981 passed (71 ignored), editor
  609 passed (66 ignored), player 11 passed, 0 failures. The first run failed
  `sample_presets_match_their_generator`: the Stormy Sky preset's excluded-field list is made
  from the volume's fields, so it gained the three new overrides; the preset was regenerated
  (it leaves them alone, as intended).
- New tests: unit (CoC thin lens and Gaussian ramp, exposure target / multiplier / frame-rate
  independent adaptation, uniform packing, override serialisation, decal culling and priority
  order, angle fade, shader validation), GPU (inactive overrides leave the image bit-identical,
  Auto Exposure off returns exactly to the fixed exposure, Gaussian and Bokeh blur out of focus
  and keep the focus sharp, motion blur from a turning camera and from a moving object only
  around it, auto exposure brightens dark and darkens bright scenes, everything with the
  upscaler, decals on meshes with opacity, priority and angle fade, textured / normal-only /
  mask-only decals, decals on terrain), kittest (volume sections, Decal Projector Inspector and
  gizmo, GameObject ▸ Rendering ▸ Decal Projector points down), `postfx_demo_matches_its_generator`.

## What's left
- Physical camera properties on the Camera (sensor, aperture, focus distance) feeding Bokeh
  ("Use Physical Camera" focus mode); today the lens is set on the volume.
- Decal layers / rendering-layer masks (a decal can't skip an object), decals on transparent
  surfaces, a Shader Graph decal target. Decal textures are decoded on the render thread the
  first time they are used (a one-off hitch per texture); views larger than the storage limit
  (about 4K at 128 MiB) draw no decals.
- Motion blur costs 0.6 ms at 8 samples (each sample reprojects its depth): a half-resolution
  velocity / depth prepack would roughly halve it.
- Local (box / sphere) volumes with blend distance, film grain, chromatic aberration and lens
  distortion (the rest of 17A).
- Auto exposure has no exposure curve remapping or debug histogram view yet.
