# Misty GI — fully dynamic global illumination (design + prototype)

Goal: realtime global illumination with **no baking**: one-plus bounces of diffuse light, sky
occlusion and (later) glossy reflections, from any mix of static and moving objects, lights and
sky. Targets: about **3–4 ms at 1920×1080 on an RTX 4060 at High**, and a cheaper **Low**. No
hardware ray tracing required (optional backend later).

It is the engine's answer to "something like Lumen, faster, without losing the look". We copy
*behaviour and published ideas* (papers, talks), never code, shaders, assets or names.

Status: design below; a working prototype of the core loop is in `render/misty_gi/` behind a
Post-process Volume override that is **off by default** (see *Prototype* and *Results*).

---

## 1. What the engine already has (and keeps)

| Piece | Where | Role in Misty GI |
|---|---|---|
| Depth + normal prepass, velocity | `render/screen_space.rs` | Input of the final gather (world position, normal per pixel). |
| SSGI (half-res cosine marches on last frame's lit colour, temporal + à-trous) | `render/screen_trace/` | **First tier of the gather**: on-screen hits keep their detail; only the rays that leave the screen fall back to Misty GI instead of the static ambient. |
| SSR (Hi-Z mirror rays + roughness blur) | same | First tier of reflections. |
| GTAO | `render/ao.rs` | Sub-voxel contact occlusion on top of the GI (probes can't see 2 cm crevices). |
| Light probes, reflection probes, lightmapper | `render/probes`, `lightmap` | Stay for projects that bake; Misty GI replaces their *diffuse* term when on. |
| Lit-shader GI hook: one packed f16 texel per pixel at mesh binding 17 | `screen_trace.wgsl` | Misty GI writes into the same texel (no new binding: the mesh pipelines are at the 16-texture limit, terrain uses all 16). |
| Pass timers | `render/pass_timer.rs` | Per-stage GPU time of every Misty GI stage. |

Units follow decision 2 of `gfx-ssgi.md`: the lit shader's ambient is `mean incoming radiance ×
albedo` and a light adds `albedo × colour × N·L`. So the GI term is the **cosine-weighted mean
of incoming radiance** (irradiance / π); no extra π anywhere.

## 2. Architecture at a glance

```
            ┌──────────────── scene (every frame, CPU: cheap list building) ─────────────────┐
            │ mesh instances (transform, mesh SDF or analytic shape, albedo, emission)       │
            │ terrains (heightfield) · tree instances (trunk capsule + crown ellipsoid)       │
            └───────────────────────────────┬──────────────────────────────────────────────────┘
 (1) Scene representation                   ▼
     Global SDF clipmap: N levels × 64³ around the camera, each level 2× the voxel size
     + material volume (albedo, emission) per level                     [dirty levels only]
 (2) Surface (radiance) cache               ▼
     Radiance voxels in a ±2-voxel band around surfaces: direct light (sun, points, spots,
     rectangles; SDF shadow rays) + emission + albedo × GI at that point (multi-bounce feedback)
 (3) Final gather                            ▼
     a. Screen-space tier: SSGI rays (existing), misses → b
     b. World-space tier: irradiance probe clipmap (24³ per level), each probe traces rays
        through the SDF, hit → radiance cache, miss → sky; SH L1, temporal hysteresis
     c. Per pixel: trilinear probes × SDF visibility + back-face weight → irradiance
 (4) Denoise: temporal in the probes (hysteresis), bilateral upsample; SSGI keeps its own
 (5) Composite: the lit shaders' existing GI texel (ambient replaced, × albedo × AO)
```

## 3. Scene representation for software tracing

### Decision: a global SDF clipmap built from per-object distance fields

- **Global SDF clipmap**: `levels` cubes of 64³ voxels (R16Float, metres), centred on the camera,
  level *k* has voxel size `v0 · 2^k` (High: `v0` = 6.25 cm, 5 levels → 4 m … 64 m wide). Traces
  sphere-step through it: start in the finest level that contains the point, move to the next
  level when leaving it. Beyond the last level a ray is "sky".
- **Built from object SDFs every time something changes** (not voxelised from triangles):
  - *Analytic shapes* for primitives (box, sphere/ellipsoid, capsule, cylinder): exact and free.
  - *Mesh SDFs* for imported meshes: a 32³ (incl. one voxel of padding) signed distance volume in
    mesh space, baked once per mesh on the CPU (BVH closest point; sign from the share of
    back-face hits of 6 rays, Lumen-style, so open meshes still get an inside), stored in a 3D
    atlas. Instances reuse it, so moving or adding instances costs nothing but a recompose.
  - *Terrain*: the heightfield itself (a downsampled R32Float copy), distance ≈ `(y − h) /
    √(1 + |∇h|²)`.
  - *Trees*: a trunk capsule and a crown ellipsoid per instance from the `.tree` recipe (crown
    shrunk to ~75 % for its porosity). Foliage is the hard case of every SDF method; proxies give
    the right *sky occlusion* under a canopy, which is what matters at this scale.
- **Material volume** next to the SDF: albedo and emission of the nearest object per voxel (one
  average colour per instance: material colour × the albedo texture's mean; terrain = mean of its
  layers). Diffuse GI needs average colour, not texture detail.
- **Updates**: a level is recomposed when its snapped centre moves (by 8 voxels) or when an object
  overlapping it changes (hash of quantised transforms and materials). The prototype recomposes
  whole levels, at most two per frame, finest first. Production: toroidal addressing so a move
  only rebuilds the new 8-voxel slab, and an object change only its bounding box.

### Why not a voxel clipmap (voxelise triangles every frame)?

| | SDF clipmap from object SDFs | Voxel clipmap (rasterised) |
|---|---|---|
| Trace cost | sphere tracing skips empty space: ~20–40 steps for 50 m | fixed-step or mip-cone marching: 2–4× more samples |
| Moving objects | recompose from cached object SDFs (no geometry work) | re-voxelise the geometry (all its triangles) |
| Pipelines to touch | none (instance list only) | a voxelisation variant of every geometry pipeline (mesh, skinned, terrain, trees, grass, custom shaders) |
| Thin geometry | needs a minimum thickness (hit epsilon ≈ ½ voxel) | conservative rasterisation keeps it |
| Skinned / deforming meshes | rigid only (use capsule proxies per bone later) | free |

The SDF route wins on cost and on staying out of the other pipelines (grass, trees and virtual
geometry are being reworked by other sessions). Skinned characters get per-bone capsules later.

### Coordination with virtual geometry

If `docs/pending/virtual-geometry.md` lands with clusters, mesh SDFs stay **per mesh, not per
cluster** (32³ is already coarser than any cluster), and the SDF baker should consume the same
CPU mesh data (`models::mesh`). The only shared need is a stable mesh key and the mesh's
bounds; no cluster data is required by Misty GI. Nanite-style meshes with millions of triangles
need the BVH bake on a worker thread (the prototype already bakes big meshes off-thread and
leaves them out of the SDF until ready).

## 4. Surface cache (what a ray sees when it hits)

### Decision: a radiance voxel band in the clipmap, not Lumen-style cards

- Per level, an Rgba16Float radiance volume (64³) and an Rgba8Snorm normal volume. Voxels within
  ±2 voxels of a surface store **outgoing diffuse radiance** = `albedo × (direct + indirect) +
  emission`; others are empty (alpha 0).
- **Direct light**: every light in the frame (directional, point, spot with the engine's exact
  falloff; rectangles as a form-factor approximation), shadowed by a **soft SDF shadow ray**
  (32–48 steps) from the surface.
- **Indirect**: `albedo × GI at that point` read from the probe clipmap of the *previous* frame:
  light bounces once more every frame, so the cache converges to multi-bounce light (decision 6
  of SSGI, here in world space).
- **Lookup**: manual 8-tap trilinear weighted by validity and by how well each voxel's normal
  faces the incoming ray, so a ray hitting the inside of a 10 cm wall doesn't average in the dark
  outside face.

### Why not cards?

Lumen's cards capture each mesh from up to 6 directions into an atlas (rasterising every mesh
with its real material), then light the atlas. They give texel-level detail for reflections, but
need card placement, an atlas allocator, capture passes through every material pipeline and a
lot of bookkeeping. For **diffuse** GI the voxel band (6–25 cm near the camera) is enough, it is
fixed-cost, has no capture passes and is trivially dynamic. Cards (or a "mesh card lite": 6
axis-aligned 64² captures per instance) become worth it only for *glossy reflections of
off-screen surfaces* — roadmap step R4.

## 5. Final gather

### 5.1 Screen-space tier first (reuse SSGI)

When the volume also has SSGI on, the compose pass combines both per pixel:
`gi = ssgi_hits × ssgi_intensity + ssgi_visibility × misty_irradiance`. On-screen detail (contact
bounce, small objects) comes from SSGI; everything that leaves the screen, is occluded or hits
nothing gets the world-space answer instead of the static ambient. With SSGI off, Misty GI alone
feeds the term.

### 5.2 World-space tier

**Prototype: irradiance probe clipmap** (DDGI-like, but traced through the SDF instead of a BVH):

- `levels` grids of 24³ probes (High), each level the same box as its SDF level (spacing
  `width / 24`: 17 cm … 2.7 m). Toroidal storage: a probe keeps its world cell, history resets
  when a new cell scrolls in.
- Each update a probe traces 32 rays (spherical Fibonacci, randomly rotated every update),
  through the SDF levels; hit → radiance cache, miss → sky radiance in that direction.
- Stored as **SH L1** radiance (12 floats). Irradiance per normal is the clamped-cosine
  convolution (`c0·Y0 + ⅔·c1·Y1(n)`), slightly windowed against ringing.
- Amortised: a quarter of the probes per frame, blended with hysteresis 0.85 → full response in
  about ½ s at 60 fps (see *Results* for moving lights).
- Per pixel (half resolution): the finest level containing the point (blending to the next one at
  the border), 8 probes trilinearly weighted × **SDF visibility** (a 6-step march from the
  surface towards each probe; probes behind a wall or inside geometry get ~0) × a soft
  back-face weight. This removes the classic probe leaks without DDGI's depth-moment atlases.

### 5.3 Radiance cascades — evaluation (the "better performance" angle)

Radiance cascades (Sannikov) split the angular/spatial domain: cascade *i* has probes spaced
`s0·2^i`, `D0·4^i` directions (3D), and traces only the interval `[l0·(4^i − 1)/3, l0·(4^{i+1} − 1)/3]`;
a ray that misses in its interval takes the radiance of the next cascade (interpolated
spatially). Penumbra condition: near light needs spatial resolution, far light needs angular
resolution, so each cascade costs about the same in 2D. In 3D the per-cascade ray count
**halves** (probes ÷8, rays ×4): the total is dominated by cascade 0.

Numbers for a 64 m outdoor clipmap at 1080p:

| Option | Rays / frame | Ray length | Notes |
|---|---|---|---|
| World-space RC, c0 dense (25 cm, 6 dirs) over 64×32×64 m | 256×128×256×6 ≈ **50 M** | short | Not viable; 3D c0 density is the killer. |
| World-space RC, c0 sparse (1 m) | 64×32×64×6 ≈ 0.8 M (+0.4 M, +0.2 M …) | short (interval) | Viable cost, but 1 m c0 = probe-grid resolution: no better than probes near surfaces. |
| Screen-space c0/c1 (probes on the depth buffer every 4 px, hemispheres) + world-space c2+ | ~0.13 M + 0.07 M + world 0.3 M | c0/c1 very short | **Best of both**: per-pixel detail from short screen-anchored intervals, long rays traced once per sparse world probe and *shared* by thousands of pixels. |
| Lumen-like screen probes (16 px, 64 rays, full length) | 0.5 M | full | Each long ray is traced per screen probe: no sharing. |
| **Prototype** probe clipmap (24³ × 5, 32 rays, ¼ per frame) | ~0.55 M | full | Simple, stable, leaks fixed by SDF visibility. |

Conclusions:

1. **Full world-space radiance cascades in 3D do not save time**: cascade 0 must be dense to beat
   probes, and dense 3D grids are 10–50× over budget.
2. The win of cascades is the **interval merge**: long rays are traced only by sparse high
   cascades and reused. That maps directly onto Misty GI's structure: the probe clipmap levels
   *are* cascades if level *k* traces only its interval and takes level *k+1*'s directional
   radiance at the interval end. That needs **directional radiance per probe** (8×8 octahedral,
   not SH L1) in the coarse levels. Expected saving: ~2–3× fewer SDF steps per frame (most steps
   are spent in the long tail of each ray), at equal quality.
3. Recommended production gather: **screen-space cascade 0–1** (or screen probes at 8 px with
   short rays) for near-field detail + **world-space cascades from level 2 up** for the far field
   and sky occlusion. Roadmap step R2/R3. The prototype's probe clipmap is the stepping stone
   (the same SDF, radiance cache, sky model and composite are reused).

### 5.4 Denoising

- **Temporal**: in probe space (hysteresis, per-probe history reset on scroll-in). Probes are
  stable by construction (no per-pixel noise), so no screen-space temporal pass is needed for the
  world tier. Moving lights lag by the hysteresis (~½ s); production adds a per-probe
  "luminance change → faster blend" rule (DDGI-style) to cut lag on sudden changes.
- **Spatial**: the per-pixel interpolation is smooth (trilinear + visibility); the half-res image
  is bilaterally upsampled in the existing compose pass. SSGI keeps its own temporal + à-trous.
- Production with screen probes / cascade 0: temporal reprojection of probe radiance + a 3×3
  probe-space filter weighted by depth/normal (as SSGI's à-trous).

## 6. Reflections (reusing the traces)

Tiered, like the diffuse gather; not in the prototype:

1. **SSR** first (existing).
2. **Rough reflections (roughness > ~0.4)**: sample the probe radiance in the reflected direction
   — needs directional radiance per probe (the octahedral maps from 5.3.2 serve both).
3. **Glossy off-screen reflections**: one SDF trace per pixel (half res, 1 ray, along a GGX
   sample) hitting the radiance cache; the hit is voxel-blurry, so fade by roughness (sharp
   mirrors keep SSR + reflection probes). Temporal + the existing roughness blur denoise it.
4. With the hardware RT backend: trace real triangles for smooth surfaces.

Cost estimate at half res: 0.5 M rays × ~24 SDF steps ≈ 0.6–0.8 ms on a 4060.

## 7. Optional hardware ray tracing backend (later)

- **wgpu ray queries** (`Features::EXPERIMENTAL_RAY_QUERY`, BLAS/TLAS through
  `create_blas`/`create_tlas`) where the adapter supports them (Vulkan, DX12 on RTX/RDNA2+). The
  trace function of every stage is behind one WGSL entry (`gi_trace(origin, dir, max_t)`), so
  the RT path swaps it for `rayQueryInitialize/Proceed` against a TLAS of the same instances (one
  BLAS per mesh, built on load; instance transforms refit per frame).
- Hits still read the **radiance cache** (material lookup by world position), so materials and
  lighting stay identical between the backends; only visibility becomes exact (thin geometry,
  skinned meshes, foliage with alpha via any-hit later).
- Fallback order: RT → SDF. A raw Vulkan/DX12 interop path (`wgpu-hal` `as_hal`) only if ray
  queries stay experimental in wgpu for too long; it is a much bigger maintenance cost.

## 8. Quality levels and fallbacks

| Level | SDF | Probes | Rays/probe, update rate | Surface light | Apply | Target (1080p, 4060) |
|---|---|---|---|---|---|---|
| **High** | 5 × 64³, v0 6.25 cm (4–64 m) | 24³ × 5 | 32, ¼ per frame | all levels each frame | half res, 6-step visibility | ≈ 3–4 ms |
| **Low** | 4 × 48³, v0 12.5 cm (6–48 m) | 16³ × 4 | 16, ¼ per frame | one level per frame | quarter res, 3-step visibility | ≈ 1–1.5 ms |
| Off | — | — | — | — | — | 0 (nothing allocated, lit shaders read the zero header) |

Fallbacks: no compute/storage support → off (ambient + light probes as today). View too large for
the texel buffer → off for that view. Transparent objects and water keep the probe / sky ambient
(they read the texel only where the prepass saw them). Lightmapped surfaces skip Misty GI unless
*Lightmapped Surfaces* is ticked (as with SSGI).

## 9. Unity-like UI

- **Lighting window ▸ Environment ▸ Global Illumination**: `None | Misty GI` (one setting per
  scene; the volume can't turn it on in a scene where it's None). *Prototype: the volume
  override alone; the Lighting-window switch is roadmap R1.*
- **Post-process Volume ▸ Misty Global Illumination** (override, blends like the others):
  Intensity (0–2), Quality (Low / High), Sky Occlusion strength, Lightmapped Surfaces, Debug view
  (Off / SDF / Radiance Cache / Probes).
- Scene view ▸ Effects dropdown follows Post Processing like every volume effect.
- Frame Debugger events: *Misty GI: Scene SDF*, *Misty GI: Surface Cache*, *Misty GI: Probe
  Trace*, *Misty GI: Gather*. Pass timers report the same names.

---

## 10. Prototype (what is implemented)

Module `crates/misty_engine/src/render/misty_gi/`:

| File | What |
|---|---|
| `settings.rs` | `MistyGiSettings` from the volume override `MistyGlobalIlluminationOverride` (off by default; Intensity, Quality). |
| `scene.rs` | Per-frame instance list from the `Frame`: analytic primitives (Cube, Sphere, Capsule, Cylinder), mesh-SDF meshes, terrains (heightfield), terrain trees (capsule + ellipsoid); albedo/emission per instance; hashes per level. |
| `mesh_sdf.rs` | CPU mesh SDF baker (32³, BVH closest point, back-face-ratio sign), cache by mesh key, small meshes baked inline, big ones on a worker thread. |
| `mod.rs` | Per-view clipmap/probe resources, dirty tracking, the four stages, integration with the screen-trace compose. |
| `shaders/gi_common.wgsl` | Bindings, clipmap addressing, SDF sampling, sphere tracing, sky model, probe SH evaluation. |
| `shaders/sdf.wgsl` | Compose one SDF level + material volume from the instance list. |
| `shaders/surface.wgsl` | Light the radiance band (all lights, SDF shadows, emission, probe feedback). |
| `shaders/probes.wgsl` | Probe trace + SH L1 projection + hysteresis. |
| `shaders/apply.wgsl` | Per-pixel gather (half res) into the image the compose pass reads. |

Edits outside the module are small: `PostSettings::misty_gi`, the volume override, the compose
pass (`in_c` = Misty GI image), the lit-shader hook (`screen_trace.wgsl`: GI × AO when Misty GI is
on), pass-timer marks and Frame Debugger events in `render/mod.rs`, the Inspector section.

## 11. Results

*(Filled in from the measurement runs; see the step log.)*

## 12. Roadmap to production

- **R1 — Product surface**: Lighting window switch, Quality ▸ Global Illumination tier per
  quality level, Inspector debug views (SDF slice, radiance cache, probe spheres), editor gizmo
  for the clipmap bounds.
- **R2 — Directional radiance + interval merge**: 8×8 octahedral radiance per probe in the coarse
  levels; level *k* traces only its interval and merges level *k+1* (radiance cascades in world
  space). Halves the SDF steps; enables rough reflections.
- **R3 — Screen-space near field**: screen probes / cascade 0–1 on the depth buffer (8 px, short
  rays) replacing the per-pixel probe interpolation near the camera: crisp contact GI without
  SSGI's dependence on last frame's colour.
- **R4 — Glossy reflections**: SDF reflection rays into the radiance cache + SSR + probes; optional
  lightweight cards for sharper off-screen reflections.
- **R5 — Incremental clipmap**: toroidal scroll (slab updates), per-object dirty boxes, brick
  culling (per 8³ brick object lists, empty-brick skipping in traces).
- **R6 — Dynamic coverage**: skinned meshes as bone capsules, per-material albedo from textures
  (mean colour), emissive textures, grass as a density layer (sky occlusion near the ground).
- **R7 — Hardware RT backend** (wgpu ray queries) behind `gi_trace`.
- **R8 — Multi-view**: share one clipmap between views that are close (Scene + Game view), so the
  editor doesn't pay twice.
