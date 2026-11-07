# Grass visibility buffer (branch grass-vis)

The grass study's winner (docs/pending/grass-study.md, B4) in the live renderer: the lit grass
pass is replaced by a multisampled visibility buffer and a compute resolve that shades each
visible blade sample once with the grass's own lighting. Same picture as `meadow-trees-1`
(within 0.07 levels, no pixel over 8), same shimmer, **−5.8 ms on High, −4.4 ms on Medium,
−2.3 ms on Low** at 1080p. Also: grass chunk uploads no longer hitch, and the grass uniforms
are no longer rebuilt every frame.

## How it works

Before: grass depth (standing blades, alpha tested, 4× MSAA) then the lit grass (every blade
and flat fan, depth test `LessEqual`, alpha to coverage, no depth writes: where fans overlap
the later one shows). The lit pass launched ~15 M fragment lanes at 1080p for ~2 M visible
samples (2×2 quads around thin leaves, one shade per covering blade per pixel under MSAA,
overlapping fans).

Now (render/grass_vis.rs, shaders/grass_vis.wgsl, grass_composite.wgsl):

1. **Grass depth**: unchanged, ends the main pass's first part (its targets are kept).
2. **Visibility raster** (own render pass, the main pass's depth tested, not written): the lit
   grass's own lists, order, vertex stage (`grass_vertex`), coverage (`grass_coverage`) and
   alpha to coverage; the fragment writes only
   - an `R32Float` target: the blade id as raw bits — bit 30 set (never a denormal or NaN:
     core WebGPU can't multisample integer targets), terrain (3 bits), blade page (3 bits),
     index in the page (21 bits);
   - an `Rgba16Float` target (target 0, so alpha to coverage reads its alpha): the first
     vertex of the fragment's triangle (flat, from the provoking vertex), front/back face,
     and the coverage.
   Same inputs, same quads, same hardware dither: each sample ends up owned by exactly the
   blade fragment that the lit pass would have left there (last drawn wins, as before).
3. **Resolve** (compute, per terrain with grass):
   - `cs_grass_classify`, a thread per pixel: the distinct blade fragments among the pixel's
     samples (same id, triangle, facing and coverage = one shade), a 3-bit-per-sample slot
     table, and one shading job per slot, compacted per workgroup (one global atomic per
     workgroup);
   - `cs_grass_shade`, a thread per job (indirect dispatch): the blade read from its page by
     id, the triangle's three card vertices evaluated as the vertex stage does
     (`grass_blade_frame` once per wind time, `grass_blade_point` per vertex), the attributes
     interpolated with perspective-correct barycentrics at the pixel centre (extrapolated
     where the centre lies outside the triangle, as the rasteriser does), the atlas gradients
     from the barycentrics of the quad neighbours (fine derivatives), then `grass_lit` —
     the lit grass fragment's own body, split out of `fs_grass` — and `shade_grass`
     (including Misty GI's `misty_ambient` hook). The blade is evaluated again at the
     previous wind time for the motion vector.
4. **Composite** (first draw of the main pass's second part, per sample): each sample a blade
   owns gets its slot's colour + coverage alpha, the glow target's coverage alpha and the
   motion vector, exactly what the lit pass wrote; other samples keep the terrain / trees.
   The grid, water, transparents, particles, effects and gizmos then draw and resolve as
   before (their depth test still uses the MSAA depth), so pass order and compositing are
   unchanged.

Supporting changes:

- **Blade pages** (render/grass_pool.rs): a terrain's blades now live in up to 4 storage
  buffers of up to 128 MiB (1.86 M blades each), first-fit runs per chunk, growing by copy,
  instead of one buffer per chunk. The resolve can then read any blade by id (no bindless
  in core WebGPU). The culled lists carry page-local indices with the page in the top 4 bits
  (`grass_fetch` masks them), so a pass binds a page once for all its chunks; the CPU path
  (no indirect first instance) binds each chunk's run of its page. MeadowDemo holds 2.6 M
  blades (190 MB) on High: 2 pages.
- The lighting bindings `shade_grass` reads (view group; lights, cascades, local shadow
  maps, soft-shadow parameters, near grass map, screen AO and screen-trace texels; grass
  uniform and atlas) are visible to compute too. Only those: the compute stage's storage
  buffer budget (8) is then AO + screen trace + 4 pages + jobs + count.
- **Sample count** = the main pass's (Quality ▸ Anti-Aliasing): 4× by default, 2× where the
  adapter has `TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES` (else 4×, as before). The images are
  made at the target's *internal* size and samples (Upscaling on: the internal size).
- Fallback: the lit pass draws the grass whenever the visibility buffer can't (switched off
  with `render::set_grass_vis(false)`, no GPU culling, a chunk not in a page, more than 8
  grass terrains, or the device refusing the pipelines / images — checked in an error scope).
- **Virtual geometry** has its own visibility raster (its own ids and targets) in the main
  pass's GPU-culled phases; the two splits compose (each pass begin knows whether the other
  split follows). No shared targets or encodings.
- Memory per target at 4×: 104 B a pixel — visibility 4 × (8 + 4) B, slots 4 × 8 B, table and
  motion 4 + 4 B, the job list 4 × 4 B (worst case) — 216 MB at 1920×1080, 54 MB at
  Performance upscaling's internal 960×540; 2×: 56 B a pixel.

## Results

### GPU time, full screen 1920×1080, RTX 4060

`grass_vis_matrix` (render/grass_vis_tests.rs): lit pass and visibility buffer in one process,
interleaved over 3 rounds, quietest round kept. "before" = `meadow_perf_matrix` on main before
this branch (same harness, same scene).

| view / level | before (main) | lit pass (this branch) | **visibility buffer** | lit, 2× MSAA | vis, 2× MSAA |
|---|---|---|---|---|---|
| wide High | 21.26 (47 FPS) | 21.23 (47) | **15.41 (65 FPS)** | 18.23 (55) | 13.44 (74) |
| wide Medium | 16.46 (61 FPS) | 16.32 (61) | **11.94 (84 FPS)** | 14.02 (71) | **9.96 (100)** |
| wide Low | 10.61 (94 FPS) | 10.33 (97) | **8.03 (124 FPS)** | 8.76 (114) | 6.26 (160) |
| behind High | 23.33 (43 FPS) | 23.15 (43) | **16.87 (59 FPS)** | 19.87 (50) | 14.88 (67) |
| behind Medium | 17.59 (57 FPS) | 17.57 (57) | **12.81 (78 FPS)** | 15.16 (66) | 10.90 (92) |
| behind Low | 11.71 (85 FPS) | 11.69 (86) | **8.60 (116 FPS)** | 9.92 (101) | 7.02 (142) |

Per pass (wide; ms):

| pass | High lit → vis | Medium lit → vis | Low lit → vis |
|---|---|---|---|
| Grass depth | 1.59 → 1.57 | 1.32 → 1.32 | 1.03 → 1.02 |
| Grass lit | 9.78 → – | 7.95 → – | 5.25 → – |
| Grass visibility (raster) | – → 1.79 | – → 1.51 | – → 1.17 |
| Grass shading (classify + shade) | – → 1.43 | – → 1.36 | – → 1.16 |
| Grass composite | – → 0.44 | – → 0.44 | – → 0.42 |
| **grass total** | **11.37 → 5.23** | **9.27 → 4.63** | **6.28 → 3.77** |
| Terrain, rest of main pass, TAA | unchanged (± 0.03) | | |

The study projected High ~5.2 ms / Medium ~4.7 ms (B4) of grass: landed on it.

**Medium at defaults: 11.9 ms (84 FPS), not yet 100.** With 2× MSAA (Quality ▸ Anti-Aliasing,
the study's B2 for Medium) it is 9.96 ms (100 FPS) — the presets stay 4× because 2× changes
the look (meadow-perf2's "Look at 2×": a fine stipple in the middle distance, crisper turf);
switching Medium to 2× is the user's call.

With **Upscaling ▸ Performance** (1080p output, 960×540 internal; wide, best of 2 rounds):
High 11.25 → **7.63 ms (131 FPS)**, Medium 8.38 → **5.24 ms (191 FPS)** (grass visibility 1.1 /
0.9, shading 0.4, composite 0.1 ms).

### Editor Game view (release, MeadowDemo in Play mode, docked Game view, VSync off)

`MISTY_TAB=Game MISTY_PLAY=1 MISTY_STATS=1 MISTY_CAPTURE_FRAME=600 MISTY_VSYNC=0
MISTY_QUALITY_LEVEL=<level>`, median frame time of the last 300 frames, main (1f8c778) and
this branch alternating, two rounds (both rounds within 0.1 ms):

| level | main | **grass-vis** |
|---|---|---|
| Low | 4.46 ms (224 FPS) | **3.17–3.26 ms (307–315 FPS)** |
| Medium | 9.42 ms (106 FPS) | **5.95–5.98 ms (167–168 FPS)** |
| High | 12.84 ms (78 FPS) | **8.58–8.60 ms (116 FPS)** |

`grassvis_editor_medium.png`: the Medium run's Game view with the Stats box.

### Look against `meadow-trees-1`

`grass_vis_shots` (the `meadow_perf_shots` setup: 1600×900, 48 frames, clocks fixed), every
level and view, diffed against the same shots rendered on main before this branch (which is
bit-identical to `meadow-trees-1`, meadow-perf2):

| shot | High mean / > 8 / max | Medium | Low |
|---|---|---|---|
| wide | 0.03 / 0.00 % / 16 | 0.03 / 0.00 % / 24 | 0.01 / 0.00 % / 24 |
| close | 0.07 / 0.00 % / 13 | 0.07 / 0.00 % / 18 | 0.03 / 0.00 % / 21 |
| behind | 0.04 / 0.00 % / 36 | 0.03 / 0.00 % / 33 | 0.02 / 0.00 % / 22 |
| oak | 0.03 / 0.00 % / 14 | 0.03 / 0.00 % / 21 | 0.01 / 0.00 % / 86 |

Mean luma per band of rows within ±0.05 everywhere. Not bit-exact: the resolve recomputes the
interpolation from the triangle (float rounding), takes the atlas gradients from barycentrics
at the quad neighbours instead of the hardware's derivatives, and packs colour to half floats
itself; a handful of isolated pixels per shot differ by more than a level (max 86 on Low oak,
which has no TAA to average such a pixel). The difference images are black at × 8 except for
those specks. Screenshots: `grassvis_look_<level>_<shot>.png` (before | after |
difference × 8).

- **Upscaling ▸ Performance** (internal 50 %): visibility buffer vs lit pass, same setup:
  mean ≤ 0.12, 0.00 % > 8 at every level and view (`grassvis_upscaled_high_wide.png`).
- **Misty GI on** (High): visibility buffer vs lit pass mean ≤ 0.07, 0.00 % > 8 (the GI
  itself moves the image by up to 0.65 levels a band, so it is really on;
  `grassvis_gi_high_wide.png`).
- The merged main without the visibility buffer (`set_grass_vis(false)`) is still
  bit-identical to `meadow-trees-1` (0 differing pixels in all 12 shots).

### Shimmer and motion

`grass_vis_motion` renders both paths in lockstep (1600×900): `still` = camera still, wind
blowing (the previous-wind evaluation runs), per-pixel temporal standard deviation of luma
over the last 16 of 64 frames in the lower half of the frame; `walk` = 1.4 m/s forward while
turning 25°/s for 90 frames.

| | still: temporal std (lit / vis) | walk: frame-to-frame change (lit / vis) | walk: lit vs vis per frame |
|---|---|---|---|
| wide High | 1.457 / 1.455 | 15.060 / 15.060 | 0.078 (worst 0.082) |
| wide Medium | 1.413 / 1.412 | 14.894 / 14.894 | 0.076 (0.080) |
| wide Low | 0.311 / 0.308 | 19.350 / 19.350 | 0.032 (0.037) |
| close High | 1.737 / 1.737 | 23.015 / 23.015 | 0.115 (0.141) |
| close Medium | 1.681 / 1.680 | 21.924 / 21.923 | 0.109 (0.133) |
| close Low | 0.496 / 0.495 | 26.602 / 26.604 | 0.043 (0.054) |

No worse anywhere (the motion vectors come from the same blade at the previous wind time).
`grassvis_motion_<level>_<shot>.png`: last walk frame, lit | vis | difference × 8.

## Grass upload hitch

Before: a finished chunk went up whole (`create_buffer_init` of its own buffer plus its cull
records); the first chunk of a frame always went, so one big chunk could take most of a frame
(scene-camera.md: over the 2 ms budget in 66–73 of 600 flying frames, up to 27–28 ms).

Now a chunk goes up in pieces of 8 k blades (0.6 MB of blades + 0.13 MB of records) written
into the blade pages; the chunk replaces its older version only when its last piece is in. A
piece starts only if the recent piece time still fits in the frame's 2 ms (the first of a frame
always goes). Room in the pages is found by a free-list; a page grows rarely (copy on the GPU).

Scene camera bench (`MISTY_CAMERA_BENCH=600`, scene_camera/bench.rs: 600 frames flying at
20 m/s while turning 30°/s, docked Scene view, High, VSync off), main and this branch run back
to back:

| | main | **grass-vis** |
|---|---|---|
| frames where one chunk upload alone took > 2 ms | 24 | **4** (all ≤ 3.4 ms) |
| longest grass upload in a frame | 7.89 ms | **3.35 ms** |
| hitches (frame > 1.5 × median) | 169 | **4** |
| frame interval median / p90 / p99 / max | 7.11 / 12.53 / 13.57 / 35.6 ms | **6.92 / 8.23 / 8.91 / 32.7 ms** |
| frame-time jitter (CV) | 37.7 % | **29.1 %** |
| Scene render CPU median / max | 2.08 / 11.90 ms | **1.67 / 5.46 ms** |
| GPU frame median / p99 | 6.92 / 13.29 ms | **6.81 / 8.59 ms** |
| still camera before flying | 12.59 ms | **7.90 ms** |

(A piece only starts within the budget, so a frame can still end a piece's length past 2 ms:
the four left stay under 3.4 ms. The single ~33 ms frame in both runs is not a grass upload;
not investigated.)
The headless `grass_vis_flight` agrees: longest upload in a frame 3.2 ms, longest piece 2.6 ms
(of which finding room 1.05 ms), on a quiet machine.

## Scene view CPU while flying

`grass_vis_flight` flies the start view at 20 m/s turning 30°/s with streaming on (1125×558,
the docked view), with the profiler's scopes: the renderer's CPU side is ~1.7–2 ms per frame
on a quiet machine (5+ ms while other sessions compile: the camera-bench run in scene-camera.md
had five cargo builds going). Per frame on High: queue submit ~0.8 ms, grass streaming ~0.5 ms
(uploads, now bounded), trees 0.2 ms, frame lists 0.1 ms, terrain 0.2 ms.

Cut: the grass passes' uniforms (camera, cascade casters, two near-map slices) were four new
buffers and four bind groups per terrain per frame (`create_buffer_init`), tracked and dropped
by wgpu; they are made once per terrain and rewritten. The blade pages bind once per page
instead of once per chunk. New profiler scopes: Render.TerrainBounds / Probes / ShadowPlan /
FrameLists, Render.Encode.Shadows / Main, Render.Submit.

In the camera bench above the Scene render's CPU side went from a median 2.08 ms (max 11.9)
to 1.67 ms (max 5.5); the editor's own ui() from 2.80 to 2.34 ms median (p99 10.3 → 7.8). The
"~6 ms" of scene-camera.md was measured with five other builds running; on today's machine
main's median is 2.1 ms, so most of what was left to cut was the upload spikes.

## Tried

- **Lighter vertex reconstruction** (vertex 0 in full, only the varying attributes of the
  other two): no measurable change (the compiler already shared the blade's work), kept for
  clarity.
- **One thread per pixel shading every slot** (first version): 2.0 ms; the two-step
  classify / job list: 1.4 ms (threads no longer wait for a neighbour that sees four blades).

## Left

- Medium at defaults is 11.9 ms (84 FPS). 2× MSAA brings it to 10.0 ms (100 FPS) at the cost
  of meadow-perf2's documented 2× look; Grass Ground Cover (off by default) would also save
  the fans' share of the raster. Both are the user's call.
- The composite runs per sample over the whole screen (0.4 ms); a stencil of grass pixels
  could skip the rest.
- The resolve's job buffer is sized for the worst case (every sample its own blade): 33 MB at
  1080p 4×.
- Velocity of samples 1–3 of a pixel whose sample 0 is not grass carries a stale value (TAA
  reads sample 0 only).
- Vendor A2C patterns: the raster keeps hardware alpha to coverage, so it matches the lit pass
  on any GPU; the 2× path needs the adapter feature (else 4×).

## Checks

Release profile only: `cargo fmt --all`; `cargo clippy --workspace --all-targets --release`:
0 warnings; `cargo test --workspace --release --no-fail-fast`: misty_engine 895 passed, 0 failed,
55 ignored (the hitch test `scripted_meadow_run_has_no_hitches` passed); misty_editor 574
passed, 1 failed (the known `sample_foliage_lighting::foliage_lighting_demo_matches_its_generator`),
61 ignored; misty_player 9 passed.

## Files

- `render/grass_vis.rs`, `shaders/grass_vis.wgsl`, `shaders/grass_composite.wgsl`: the
  visibility buffer.
- `render/grass_pool.rs`: blade pages.
- `render/terrain_grass.rs`: chunks in pages, piecewise uploads, page groups.
- `render/grass_cull.rs`, `shaders/grass_cull.wgsl`: lists carry page indices.
- `shaders/grass_blade.wgsl`: `grass_blade_frame` / `grass_blade_point` (same arithmetic as
  before, split), `grass_fetch` masks the page bits; `shaders/grass.wgsl`: `grass_out`,
  `grass_lit` split out of the vertex and fragment stages.
- `render/mod.rs`: main pass split, `grass_vis_active`, passes; `render/terrain.rs`: cached
  grass uniforms.
- Harnesses (ignored, release): `cargo test --release -p misty_engine --lib grass_vis_ --
  --ignored --nocapture --test-threads=1` — `grass_vis_shots` (`MISTY_VIS=0`,
  `MISTY_UPSCALING=performance`, `MISTY_GI=1`, `MISTY_MSAA2=1`, `MISTY_QUALITY`,
  `MISTY_GFX_SHOT`, `MISTY_GFX_DIR`), `grass_vis_matrix` (`MISTY_SETUPS=lit,vis,lit2,vis2`),
  `grass_vis_motion`, `grass_vis_flight` (`MISTY_FLY_SPEED`, `MISTY_FRAMES`, `MISTY_SIZE`),
  `grass_vis_resident`. Not ignored: `grass_vis_matches_the_lit_pass`,
  `pool_reuses_runs_grows_and_opens_pages`, the shader validation.
