# Misty GI — fully dynamic global illumination (design + prototype)

Goal: realtime global illumination with **no baking**: one-plus bounces of diffuse light, sky
occlusion and (later) glossy reflections, from any mix of static and moving objects, lights and
sky. Targets: about **3–4 ms at 1920×1080 on an RTX 4060 at High**, and a cheaper **Low**. No
hardware ray tracing required (optional backend later).

It is the engine's answer to "something like Lumen, faster, without losing the look". We copy
*behaviour and published ideas* (papers, talks), never code, shaders, assets or names.

Status: design below; a working prototype is in `render/misty_gi/` behind a Post-process Volume
override that is **off by default** (see *Prototype* and *Results*): within 1–2 sRGB levels of the
path-traced lightmap in the Cycles room, grass and trees included, glossy reflections, works with
Upscaling; High ≈ 1.6–1.9 ms (+1.8 / +2.7 ms per frame), Low ≈ 0.3 ms (1080p, RTX 4060).

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
     b. World-space tier: irradiance probe clipmap (24³ per level at High), each probe traces rays
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
- Amortised: a quarter of the probes per frame, blended with hysteresis 0.85; an update that
  changes a probe's brightness by more than 25 % blends at 0.5 instead (lights switched or
  moved). A new probe averages its first updates (1/n) before the hysteresis takes over.
- Probe rays only hit surfaces they close in on, and not within one voxel of the probe: a
  probe beside a wall must not see that wall in every direction (on coarse levels the hit
  epsilon is larger than the probe's distance to the wall).
- Per pixel (half resolution): the finest level containing the point (blending to the next one at
  the border), 8 probes trilinearly weighted × **SDF visibility** (a 4-step march from the
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
| **Prototype** probe clipmap (24³ × 5, 32 rays, ½–¼ per frame, interval merge) | ~0.8 M | ½ level, then merged | Simple, stable, leaks fixed by SDF visibility. |

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
  world tier. Moving lights: half-way to the new bounce in ~18 frames, 90 % in ~80 (the
  radiance cache and the probes feed each other, so the last bounces settle slowest).
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

| Level | SDF | Probes | Rays/probe, update rate | Gather | Measured (1080p, 4060) |
|---|---|---|---|---|---|
| **High** | 5 × 64³, v0 6.25 cm (4–64 m wide) | 24³ × 5 | 32; ½ per frame (levels 0–1), ¼ (coarser) | half res, 4-step visibility, glossy reflections | 1.6–1.9 ms (frame +1.8 / +2.7 ms) |
| **Medium** | 4 × 64³, v0 9.4 cm (6–48 m) | 16³ × 4 | 24; ½ / ¼ per frame | half res, 3-step visibility, glossy reflections | ≈ 0.7 ms |
| **Low** | 4 × 48³, v0 12.5 cm (6–48 m) | 16³ × 4 | 16, ¼ per frame | quarter res, 3-step visibility | ≈ 0.3 ms |
| Off | — | — | — | — | 0 (nothing allocated, lit shaders read the zero header) |

The surface cache relights every level every frame in the prototype (0.1–0.8 ms); production
relights one level per frame at Low (roadmap R5).

Fallbacks: no compute/storage support → off (ambient + light probes as today). View too large for
the texel buffer → off for that view. Surfaces the prepass didn't see (transparent materials,
vertex-displaced custom shaders) and water read the world lookup of the probe clipmap instead of
the per-pixel texel. Lightmapped surfaces skip Misty GI unless *Lightmapped Surfaces* is ticked (as
with SSGI).

## 9. Unity-like UI

Three places, kept minimal:

- **Lighting window ▸ Scene ▸ Realtime Lighting ▸ Realtime Global Illumination** (checkbox, per
  scene, saved in the scene's lighting settings; off by default): Misty GI for every camera of
  the scene at full intensity. Undoable, marks the scene dirty.
- **Project Settings ▸ Quality ▸ Global Illumination** (Off / Low / Medium / High per quality
  level; presets Low → Low, Medium → Medium, High and Ultra → High): the highest quality Misty GI
  runs at on that level. Off turns it off whatever the scene or a volume says (like Ambient
  Occlusion's tier).
- **Post-process Volume ▸ Misty Global Illumination** (override): ticking it turns GI on inside
  the volume with its Intensity (0–2), Quality (Low / Medium / High, capped by the tier) and
  Lightmapped Surfaces; it takes precedence over the scene switch. *Planned: debug views (SDF /
  radiance cache / probes).*

Making Misty GI the default for new scenes is then one line: the scene switch's default.
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
| `mod.rs` | World state (clipmaps, probes) pooled in the renderer and shared by a secondary view near a primary one; per-view gather images; dirty tracking, the four stages, integration with the screen-trace compose. |
| `shaders/gi_common.wgsl` | Bindings, clipmap addressing, SDF sampling, sphere tracing, sky model, probe SH evaluation. |
| `shaders/sdf.wgsl` | Compose one SDF level + material volume from the instance list. |
| `shaders/surface.wgsl` | Light the radiance band (all lights, SDF shadows, emission, probe feedback). |
| `shaders/probes.wgsl` | Probe trace with the interval merge, SH L1 projection, hysteresis (faster on big changes). |
| `shaders/apply.wgsl` | Per-pixel gather (half res) and glossy reflection rays into the images the compose pass reads. |

Edits outside the module are small: `PostSettings::misty_gi`, the volume override, the scene
switch (`RenderSettings::realtime_gi`, resolved in `render_scene::post_settings`), the Quality
tier (`QualityLevel::global_illumination`, applied in `Renderer::render`), the compose pass
(`in_c` = irradiance, `in_d` = glossy reflections behind SSR's), the lit-shader hooks in
`screen_trace.wgsl`, the world-lookup fallback in `mesh.wgsl`'s `shade` (transparent and custom
surfaces), water's ambient and reflection in `water.wgsl`, pass-timer marks and Frame Debugger
events in `render/mod.rs`, the Inspector section, the Lighting-window checkbox, the Quality row,
the editor marking its Scene view secondary, and one line each in `grass.wgsl` and `tree.wgsl`.

### Coverage

| Surface | How it gets Misty GI |
|---|---|
| Standard / Cutout meshes, terrain | Per-pixel GI texel (prepass + gather + compose). |
| Lit custom shaders and Shader Graphs (opaque) | The same `shade()` as the Standard shader: the texel. |
| Transparent materials, transparent or vertex-displaced custom shaders | Not in the prepass: `shade()` falls back to the world lookup. |
| Grass | `misty_ambient` (texel where the prepass saw the blade, else its own ambient). |
| Tree crowns, impostors | `misty_ambient_world` (texel, else world lookup). |
| Water | World lookup at the surface (body light, foam) and in the reflection direction: the sky reflection is scaled by GI ÷ sky ambient that way (darker under a bridge, tinted beside a lit wall); SSR still wins where it finds something. |
| Unlit shaders, particles | None (by design). |

### Multi-view: Scene and Game view share one clipmap

The world state (SDF, emission, radiance cache, probes, instance and light buffers) lives in a
pool in the renderer; each view keeps only its gather images. The editor marks its Scene view
*secondary* (`RenderTarget::set_misty_gi_secondary`). A secondary view whose camera is within a
quarter of the third-coarsest level's width of a primary view's (8 m at High, 6 m at Medium/Low),
and whose primary updated the world in the last 250 ms, runs only its gather against that world:
no SDF, surface or probe passes. Farther away, or with the Game view hidden, it keeps a world of
its own; worlds unused for 3 s are freed. The Game view is never degraded by the Scene view.

### GI for shaders with their own lighting (grass, trees, a visibility-buffer resolve)

`screen_trace.wgsl` (included by every lit pipeline, mesh group binding 17) exposes:

| Function | Cost | Use |
|---|---|---|
| `misty_ambient(hemisphere, world)` | 2 storage reads | Dense surfaces (grass): the per-pixel GI texel where the prepass saw this surface, else the shader's own ambient. |
| `misty_ambient_world(hemisphere, world, n)` | texel, else 32–64 reads | Sparser surfaces (tree crowns): texel first, else the world lookup. |
| `misty_world_irradiance(world, n)` | 32–64 storage reads | World-space probe lookup, no texel needed: irradiance × intensity, w = 0 outside the probe grids or with Misty GI off. |

The world lookup reads a copy of the probe clipmap that Misty GI writes after the texels each
frame (layout in `screen_trace.wgsl`). For the grass visibility-buffer resolve (branch
`grass-vis`): bind the same buffer and call `misty_ambient` (cheap) per pixel; the world lookup
per grass pixel cost about 14 ms in MeadowDemo when it was tried, so use it only per blade or
for sparse pixels.

## 11. Results

### Cost (RTX 4060, 1920×1080, Quality ▸ High, GPU pass timestamps, shared GPU-timing lock)

`misty_gi_scene_pass_times` (release, median of 60 frames after 60 to settle; the scene still,
so the SDF stage doesn't run):

| Scene | Misty GI | Frame | Misty GI total | Surface cache | Probe trace | Gather (+ reflections) |
|---|---|---|---|---|---|---|
Round 16 (main effbe2f: grass visibility buffer, virtual shadow maps available):

| Scene | Misty GI | Frame | Misty GI total | Surface cache | Probe trace | Gather (+ reflections) |
|---|---|---|---|---|---|---|
| CyclesRoomDemo | off | 8.62 ms | — | — | — | — |
| | Low | 8.93 | **0.28** | 0.05 | 0.11 | 0.11 |
| | Medium | 9.61 | **0.74** | 0.08 | 0.28 | 0.38 |
| | High | 10.38 | **1.91** | 0.08 | 1.41 | 0.42 |
| MeadowDemo | off | 15.51 | — | — | — | — |
| | Low | 16.49 | **0.32** | 0.11 | 0.10 | 0.12 |
| | Medium | 17.06 | **0.68** | 0.20 | 0.21 | 0.27 |
| | High | 17.88 | **1.60** | 0.21 | 1.07 | 0.32 |

- Whole-frame cost at High: **+1.8 ms** (room) and **+2.4 ms** (meadow), inside the 3–4 ms target.
  In MeadowDemo the rest of the difference is the full-resolution compose (0.35 ms, the scene had
  no SSGI/SSR before) and the grass reading its GI texel. (Phase 2 measured the meadow at 21.4 →
  24.1 ms before the grass visibility buffer; Misty GI's own passes are unchanged.)
- **Editor, Game + Scene view** (`misty_gi_scene_shared_views`, both 1080p High, the Scene view
  1.9 m from the Game camera): sharing the Game view's clipmaps saves **1.1 ms per editor frame**
  in both scenes (MeadowDemo 34.3 → 33.2 ms per pair, CyclesRoomDemo 17.1 → 15.9 ms): the Scene
  view runs only its gather.
- **Interval merge** (radiance-cascades style, section 5.3): fine probes trace half their level's
  width and take the coarser probes' radiance in the same direction beyond it. High's probe
  trace went from 2.0 to 1.4 ms (room) and 1.1 ms (meadow), and moving lights settle faster
  (coarse information reaches the fine probes immediately).
- With only Misty GI on (no SSGI/SSR), the lit colour history copy and the main-pass split are
  skipped.
- **Moving object** (`misty_gi_scene_moving_object_cost`): scene SDF recompose 0.07 ms per frame.
- Grass first used the world lookup per fragment: +14 ms in MeadowDemo (32–64 storage reads per
  blade fragment). It now reads the per-pixel texel (2 reads).
- Early runs while other sessions were timing on the same GPU read 3–4 ms for the first High;
  those were contention. Every number above was taken holding the shared GPU-timing lock.

### Quality: CyclesRoomDemo against the path-traced lightmap

`render_cycles_room_misty_gi` (editor crate). Same camera, light, exposure and AO as the baked
reference; Misty GI replaces the bounce light, nothing is baked.
`docs/screenshots/gfx/gi_cycles_room_compare.png` (top: probes / Misty GI / Misty GI + SSGI /
path-traced lightmap; bottom: red-green room with SSGI only / Misty GI / lightmap).

Mean sRGB brightness of 21×21 patches:

| | ceiling | back wall | left wall | floor front | pillar front | cube front | floor right of cube |
|---|---|---|---|---|---|---|---|
| Room probes + SSGI (shipped) | 196 | 179 | 179 | 159 | 115 | 50 | 69 |
| **Misty GI** | 197 | 191 | 178 | 161 | 104 | 47 | 61 |
| Misty GI + SSGI | 197 | 190 | 178 | 162 | 108 | 49 | 60 |
| **Path-traced lightmap** | 197 | 189 | 179 | 163 | 105 | 44 | 71 |
| Red/green: SSGI only | 190 | 178 | 99 | 154 | 27 | 0 | 45 |
| **Red/green: Misty GI** | 194 | 186 | 103 | 157 | 65 | 23 | 40 |
| **Red/green: lightmap** | 194 | 184 | 103 | 158 | 65 | 16 | 45 |

- Misty GI is within 1–2 levels of the path tracer on the ceiling, walls, floor, pillar and
  cube: closer than the baked room probes (which flatten the back wall and over-light the
  camera-facing box fronts), with nothing baked.
- Colour bleeding: the pillar's red-lit face matches the lightmap exactly (65 vs 65); the cube
  front is a little brighter (23 vs 16) and the floor right of the cube a little darker (61 vs
  71: it sits in the cube's contact shadow, which GTAO deepens too).
- Same numbers with Upscaling ▸ Performance (`misty_gi_works_with_upscaling`): Misty GI runs at
  the internal resolution.
- The first version centred the clipmap on the camera, 3 m outside the room: the room fell into
  the 25 cm level and the box fronts came out far too dark (66 / 11). Centring each level a
  quarter of its width ahead of the camera fixed it.

### Quality: MeadowDemo (grass and trees receive GI)

`misty_gi_scene_meadow_shots` → `docs/screenshots/gfx/gi_meadow_<shot>_compare.png` (left off,
right Misty GI High): `grove` (beside the nearest tree, midday), `grove_golden` (same spot, golden
hour), `grove_bare` (grass off, terrain ground), `wide_midday`, `wide_golden`.

Mean sRGB of the ground in the canopy's shade and of the crown (`grove*`):

| Shot | Shade, off | Shade, Misty GI | Crown, off | Crown, Misty GI |
|---|---|---|---|---|
| grove (midday) | (5, 14, 2) | (9, 18, 2) | (51, 52, 46) | (54, 56, 45) |
| grove golden | (96, 102, 20) | (96, 102, 18) | (35, 37, 33) | (36, 39, 33) |
| grove, grass off | (15, 24, 23) | (24, 28, 18) | (51, 52, 46) | (54, 56, 45) |

- Under the canopy the blue sky light falls (sky occluded by the crown proxies) and warm bounce
  from the sunlit meadow comes in: shaded grass turns from near-black blue-green to a warmer
  green; on bare ground the blue drops by a quarter while red rises by half.
- The effect is **subtle in the shipped meadow**: the grass shader's own occlusion terms crush the
  ambient in the turf to near black (sRGB ≈ 5 / 14 / 2 in shade with GI off), and whatever ambient
  Misty GI supplies is multiplied down the same way. The scene's sun-to-ambient balance was tuned
  without GI; retuning it (more ambient, less turf occlusion) is an art decision left to the user.
- The wide shots change by ±1 level: in the open Misty GI reproduces the hemisphere ambient by
  design, and the trees are 50 m+ away.
- Tree crowns are ellipsoid proxies at 75 % size, so the canopy's shade is soft.

### Meadow ambient options (for the user to pick; scene defaults unchanged)

`misty_gi_scene_meadow_options` → `docs/screenshots/gfx/gi_meadow_opt_<shot>_compare.png` for
`wide`, `grove`, `wide_golden`, `grove_golden`: **A** Misty GI off (today), **B** on with the scene
as it is, **C** on with Lighting ▸ Environment ▸ Intensity lowered to 0.6, **D** on with the volume's
GI Intensity at 1.6. Mean sRGB of the canopy shade (`grove`) / foreground grass (`wide`):

| Shot | A off | B on | C ambient 0.6 | D GI Intensity 1.6 |
|---|---|---|---|---|
| grove | (45, 60, 11) | (47, 61, 10) | (45, 57, 9) | (51, 68, 12) |
| grove golden | (96, 106, 18) | (98, 106, 17) | (96, 104, 16) | (100, 110, 19) |
| wide | (96, 98, 34) | (97, 98, 34) | (96, 96, 32) | (99, 101, 36) |
| wide golden | (111, 103, 40) | (112, 103, 39) | (110, 100, 37) | (113, 105, 42) |

- Lowering the ambient (C; 0.4 was tried too) only darkens the meadow: Misty GI's sky light is the
  same environment ambient, so it scales down with it, and the bounce off the dark turf is small.
  It is not the knob that lets the GI show here.
- Raising the GI's own Intensity (D) lifts the canopy shade and warms it (sky occlusion and
  meadow bounce scaled together) without touching the open meadow's sun-lit look much.
- What really hides the GI in this scene is the grass shader's own occlusion of the ambient in
  the turf (any ambient, GI or not, is multiplied down there). Easing that is a shader change, not
  a scene setting, and was left alone.
- With virtual shadow maps (`gi_meadow_opt_grove*_vsm_compare.png`) Misty GI adds no light inside
  the virtual shadows; its sky occlusion darkens the canopy shade slightly, as with cascades.

### Behaviour

- **Moving light** (`misty_gi_follows_a_moving_light`): sun swung from the red wall to the back
  wall; the floor's red tint is half-way to its new value after 10 frames and 90 % after 22
  (first prototype: 18 and 80). Probes update twice as often near the camera, big changes
  blend in at 0.5, and the interval merge passes coarse changes straight to the fine probes.
- **Moving object**: the levels holding it recompose (0.07 ms); its bounce and occlusion follow
  as the probes update (the same ~10 frames to half-way).
- **Glossy reflections** (`misty_gi_reflects_the_cube_without_ssr`): a red cube shows in a glossy
  metal floor with SSR off (mirror image red 139 / 7 / 2 against the sky's 115 / 135 / 166).
  With SSR on, SSR's sharper hits win and Misty GI fills the rest (off-screen, behind objects).
- **Trees** (`misty_gi_reaches_tree_crowns`): an oak beside a sunlit red wall turns red on the
  side facing it (redness −2 → +17).
- **Off-screen light** (`misty_gi_bounces_light_from_off_screen`): a sunlit red wall behind the
  camera still tints the floor in front of it (redness 27 against 1 with a grey wall).
- **Sky occlusion** (`misty_gi_occludes_the_sky_under_a_roof`): floor under a 3 m roof at 1.2 m
  loses about 35 % of its blue sky light; open floor keeps its ambient within ±5 %.
- **Water** (`misty_gi_shades_water_under_a_roof`): a lake under a wide roof darkens from 90 to 62
  (sRGB luminance) as its sky reflection and body light lose the sky; open water 141 → 141.
- **Custom shaders** (`misty_gi_lights_custom_shader_surfaces`): a lit `.shader` floor takes the red
  wall's bleed like the Standard shader (redness −15 → +52); a transparent custom cube (not in
  the prepass) takes it through the world lookup (redness −3 → +52).
- **Virtual shadow maps** (`misty_gi_sun_agrees_with_virtual_shadows`): with VSM on, a pillar's
  shadow on the floor is dark both on screen (74 vs 184 lit) and in Misty GI's surface cache (0.08
  vs 0.51 green radiance); the bounce in the shadow is the same as with cascades (+18).
- **Shared clipmaps** (`misty_gi_scene_view_shares_the_game_views_clipmaps`): a secondary view
  near the primary one keeps one world and still shows the bleed; 60 m away it gets its own.

### Bugs found on the way (kept as tests or comments)

- `normalize(probe − point)` with the gather point exactly on a probe (Low's grids align with
  surfaces at half a voxel) gave NaN, which `min(x, 64)` turned into a blinding 64. Guarded
  division, and the Low test now checks for a real red tint instead of "redder than before".
- The screen-trace compose only ran when SSGI or SSR was on: with Misty GI alone, the GI texel
  stayed zero (black ambient).
- Probe rays treated "within the hit epsilon" as a hit even when running alongside a wall; on
  coarse levels the epsilon exceeds a probe's distance to a wall. Hits now require the ray to
  close in on the surface and to be at least one voxel from the probe.

## 12. Roadmap to production

- **R0 — Coverage**: done (grass, trees, water, custom shaders and Shader Graphs, transparent
  surfaces; section 10). Left: per-blade world lookup for grass beyond the prepass range.
- **R1 — Product surface**: Lighting window switch and Quality tier done (section 9). Left:
  Inspector debug views (SDF slice, radiance cache, probe spheres), editor gizmo for the clipmap
  bounds, and flipping the scene switch's default once the user picks a meadow option.
- **R2 — Directional radiance**: the interval merge is in (with SH L1 radiance, which is blurry
  in direction); 8×8 octahedral radiance per probe in the coarse levels would sharpen the far
  field and give rough reflections from the probes.
- **R3 — Screen-space near field**: screen probes / cascade 0–1 on the depth buffer (8 px, short
  rays) replacing the per-pixel probe interpolation near the camera: crisp contact GI without
  SSGI's dependence on last frame's colour.
- **R4 — Reflections**: glossy SDF reflections are in (behind SSR); temporal accumulation and a
  GGX-distributed ray for rougher surfaces, and optional lightweight cards for sharper
  off-screen reflections.
- **R5 — Incremental clipmap**: toroidal scroll (slab updates), per-object dirty boxes, brick
  culling (per 8³ brick object lists, empty-brick skipping in traces).
- **R6 — Dynamic coverage**: skinned meshes as bone capsules, per-material albedo from textures
  (mean colour), emissive textures, grass as a density layer (sky occlusion near the ground).
- **R7 — Hardware RT backend** (wgpu ray queries) behind `gi_trace`.
- **R8 — Multi-view**: done for the editor (section 10, 1.1 ms saved per editor frame). Left:
  split-screen game cameras (several primaries) could share by the same rule.
