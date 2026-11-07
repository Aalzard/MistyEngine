# MeadowDemo performance, round 2 (branch meadow-perf2)

Goal: ≥ 100 FPS (≤ 10 ms GPU) full screen 1920×1080 on Medium with the trees, Low well above,
High as close as possible. Two tracks kept apart: **A** = engine changes that leave the picture
pixel-identical to `meadow-trees-1`; **B** = small look changes behind switches, off by default,
for the user to pick.

## Measuring

- `render/meadow_perf_tests.rs` (all `#[ignore]`, release):
  `cargo test --release -p misty_engine --lib meadow_perf_ -- --ignored --nocapture --test-threads=1`
  - `meadow_perf_shots`: every level (`MISTY_QUALITY=high,medium,low`) from four views — wide
    (the player's start view), close (crouching into the grass), behind (turned round to the
    trees behind the player), oak (towards the broad oak) — 1600×900, 48 frames, into
    `MISTY_GFX_DIR`. `MISTY_PERF_OPTS=cover` turns Track B's ground cover on.
  - `meadow_perf_times`: GPU frame median, `Renderer::render` CPU time and the per-pass GPU
    times (round 1's `breakdown`), per level and view (`MISTY_GFX_SHOT=wide,behind`).
  - `meadow_perf_ab`: A/B in one process, the two setups alternating over 3 rounds, medians
    (`MISTY_AB=cull` GPU grass culling off/on, `MISTY_AB=cover` ground cover off/on). Other
    agents ran GPU tests and builds on the same machine during this work; alternating keeps
    both sides under the same load, and a single run's per-pass numbers can show spikes.
  - `meadow_perf_counts` (CPU replay of the chunk prefixes and the blade tests) and
    `meadow_perf_cull_counts` (reads the culled counts back from the GPU).
- **Renders are now deterministic**: clouds and froxel fog took their animation clock from
  the wall clock; with a fixed grass clock (`Environment::grass_time`, tests and recordings
  only) they now use it too. Two renders of the same build are bit-identical (round 1 had a
  noise floor of mean 0.56 / 0.34 % > 8 levels), so Track A is checked for exact equality.

## Where the time was (before, `meadow-trees-1`)

GPU ms per pass, 1920×1080, the player's start view (wide) / turned round (behind):

| pass | High | Medium | Low |
|---|---|---|---|
| Shadow maps (cascades + grass casters) | 1.28 / 1.45 | 0.56 / 0.57 | – |
| Grass near shadow map | 1.48 / 1.48 | 0.75 / 0.75 | – |
| Clouds | 0.54 | 0.54 | 0.36 |
| AO prepass + GTAO | 1.50 / 1.83 | 1.08 / 1.55 | 0.46 / 0.80 |
| Shadow mask / denoise | 0.97 | – | – |
| Sky | 0.31 | 0.31 | 0.32 |
| Terrain + trees | 2.62 / 3.75 | 2.41 / 2.96 | 2.08 / 2.42 |
| Grass depth | 1.82 / 2.19 | 1.48 / 1.74 | 1.10 / 1.28 |
| **Grass lit** | **10.41 / 11.25** | **8.35 / 9.26** | **5.38 / 6.26** |
| Rest of main pass | 0.35 | 0.35 | 0.35 |
| Sun shafts | 1.17 / 0.96 | 0.91 / 0.67 | – |
| TAA | 0.50 | 0.50 | – |
| Bloom, tonemap, finish | 0.18 | 0.18 | 0.17 |
| **GPU frame (median)** | **23.0 / 26.7** | **18.3 / 20.1** | **11.4 / 13.0** |

Two experiments decided the plan:

- **Vertex vs fragment**: with a 1×1 scissor over the grass passes (no fragments), grass
  depth still cost 0.99 / 0.75 / 0.49 ms and grass lit 2.43 / 1.68 / 1.11 ms
  (High / Medium / Low); with every blade thrown away at the top of the vertex stage, 0.98
  and 1.58 ms. So ~1 ms per million drawn instances in the depth pass and ~1.6 in the lit
  pass go to blades that draw nothing: the cost is the instance count (10 vertices, 8
  degenerate triangles each), not the shader.
- **How many instances draw nothing** (CPU replay, confirmed by GPU readback): the wide view
  on High draws 0.96 M blades per camera pass, of which 0.42 M are wholly outside the view
  (the chunks around the camera are half behind it) and 0.25 M are flat fans; the depth pass
  needs only 0.30 M, the lit pass 0.55 M. The shadow passes are worse: the near grass map's
  outer slice drew 0.67 M for 0.16 M that land in it, the cascades 0.57 M for 0.07 M.
- **Hi-Z occlusion would not pay**: only 0.06 M of the 0.55 M lit blades (11 %, wide; 7 %
  behind) are hidden behind the terrain, and their fragments already fail early Z (the
  terrain is drawn first). Building a Hi-Z of the terrain before the grass needs the MSAA
  main pass split (store + reload of four 4× targets). Dropped; numbers kept here.

## Track A: GPU grass culling (merged look-safe)

`render/grass_cull.rs`, `shaders/grass_cull.wgsl`. Before the frame's first pass, one compute
pass tests every blade that any grass pass would draw:

- A **job** is one chunk with a **lane** per pass that draws it (lit, depth, AO prepass, up
  to four sun cascades, the two near-map slices), so each blade's record is read once. The
  tests mirror grass_blade.wgsl exactly — `grass_outside` (side planes, the near plane for
  camera passes, the card's reach) and `grass_flat` (draw distance, the blade's key past its
  distance's share, caster range, the near map's inner square) — with a 1e-3 relative safety
  margin, a height rounded up and a key rounded down, so a blade is only dropped when the
  vertex stage would draw nothing of it.
- **Order is kept**: `cs_count` writes each workgroup's 256-bit visibility mask and count per
  lane, `cs_scan` (one workgroup) turns counts into offsets and writes every lane's
  `DrawIndirectArgs`, `cs_write` writes the compact index lists from the masks. The flat
  fans have no depth writes in the lit pass, so their overlap order is part of the picture:
  keeping it is what makes the result bit-identical.
- Blades now live in storage buffers; the vertex stages fetch them through the draw's list
  (`grass_fetch`, group 3 per chunk). Each pass draws its chunks with `draw_indirect`, same
  chunk order and prefixes as before (CPU side unchanged: `Grass::pass_chunks`).
- Cull records (root, height, key: 16 bytes a blade) sit in one arena buffer per terrain
  (first-fit free list, grows by copying); it stays under the default 128 MB binding limit.
- Without `INDIRECT_FIRST_INSTANCE` (or a full arena) the passes draw every blade of the
  prefix through an identity list, as before.

Also exact, small: the camera pass draws terrain chunks front to back, and the AO prepass
no longer draws terrain chunks outside the view.

## Track B1: Grass Ground Cover (Quality ▸ Grass Ground Cover, off in every preset)

The flat fans (blades the LOD shrank to height 0, drawn as small fans on the ground by the
lit pass only) cost ~3 ms of the lit grass on High even after culling: they have no depth
writes, so overlapping fans are all shaded. With the switch on:

- the lit grass leaves them out too (the culling's lit lane gets the flat test; without GPU
  culling the vertex stage skips them: `GrassParams::with_ground_cover`, `draw.y`);
- the terrain shader multiplies the lit colour of the ground under grass by a fitted factor
  in the band where fans lay (terrain_layers.wgsl `grass_ground_cover`, applied in
  `fs_terrain` after lighting and fog): from 0.8× to 2× the grass's full-density distance it
  fades in, from 0.55 × 7× to 7× it fades out, 50 % of the ground takes the fans' colour —
  darker and warmer when the camera looks into the sun ((0.78, 0.75, 0.60) × the ground),
  lighter when the sun is behind it ((1.35, 1.30, 1.20)), as the grass-lit fans read against
  the terrain-lit ground. Values in terrain.rs `GROUND_COVER` / `GROUND_TINT` /
  `GROUND_FRONT`, fitted with `MISTY_GROUND_COVER` sweeps against the row profile of the
  fans' effect (base − no fans) in the wide (backlit) and behind (front-lit) shots.
- An albedo tint was tried first: it barely moves the backlit view (the gaps' colour there is
  mostly fog and in-scattered light), and an analytic fan-density model put the effect at the
  wrong distances. Both dropped.

## Track B2: Quality ▸ Anti-Aliasing is honoured (2× or 4×)

Every level rendered 4× MSAA whatever its Anti-Aliasing said (`SAMPLE_COUNT` was a constant).
Now the renderer follows the setting:

- `render::sample_count()` replaces the constant; each `Renderer` keeps the samples it was
  built for and sets them before it builds pipelines or targets (so a second renderer, e.g.
  the material preview, stays consistent). `Renderer::set_quality` rebuilds the renderer when
  the level asks for another sample count (pipelines and GPU caches: a short hitch, meshes,
  textures and grass load again over the next frames). Targets record their samples;
  `Renderer::target_fits` tells the hosts (editor viewport, player) to make new ones, and a
  render into a stale target is skipped.
- 2× of the Rgba16Float targets is not guaranteed by WebGPU: the device now requests
  `TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES` when the adapter has it (else 2× falls back to
  4×). **Disabled renders 2×** for now: 1× needs non-multisampled variants of the depth /
  velocity readers (TAA, Hi-Z, glow, sun shafts, scene copy, UI, custom shaders) and a main
  pass without a resolve.
- To keep today's picture by default, the Low and Medium presets now say 4× (they said
  Disabled / 2× but rendered 4×). The user picks 2× per level in Project Settings ▸ Quality.
- **Look at 2×** (`perf2_msaa2_*`): mean 1.9–4.3 levels off, 12–25 % of pixels > 8 levels on
  High / Medium, 19–34 % on Low (no TAA there). Blade edges and the alpha-to-coverage
  translucency get half the coverage steps: the middle distance shows a fine stipple/checker
  under the TAA noise (`perf2_msaa2_medium_wide_zoom.png`), the turf reads a little darker
  and crisper close up; on Low, without TAA, it is visibly more aliased
  (`perf2_msaa2_low_wide_zoom.png`).

## Results

GPU frame time, 1920×1080, RTX 4060, quietest of 3 interleaved rounds per setup
(`meadow_perf_matrix`; other agents shared the GPU). "No culling" = this branch with GPU
culling off, within ±0.3 ms of `meadow-trees-1` (the before table above was measured on
`meadow-trees-1` itself, earlier and noisier).

| view / level | meadow-trees-1 | no culling | **Track A** (default) | + Ground Cover | + 2× MSAA | + both |
|---|---|---|---|---|---|---|
| wide High | 23.0 | 21.9 | **20.3** (49 FPS) | 17.4 (57) | 17.5 (57) | 15.5 (65) |
| wide Medium | 18.3 | 16.9 | **15.8** (63 FPS) | 13.3 (75) | 13.5 (74) | **11.7 (86)** |
| wide Low | 11.4 | 10.4 | **10.1** (99 FPS) | 8.5 (118) | 8.5 (117) | 7.6 (131) |
| behind High | 26.7 | 25.2 | **23.1** (43 FPS) | 19.8 (50) | 19.8 (50) | 17.6 (57) |
| behind Medium | 20.1 | 19.3 | **17.8** (56 FPS) | 14.9 (67) | 15.1 (66) | 13.2 (76) |
| behind Low | 13.0 | 12.2 | **11.6** (86 FPS) | 9.4 (107) | 9.6 (104) | 8.2 (122) |

Per pass, Track A (default), wide / behind:

| pass | High | Medium | Low |
|---|---|---|---|
| Grass culling (new) | 0.40 / 0.45 | 0.27 / 0.32 | 0.11 / 0.14 |
| Shadow maps (cascades + grass casters) | **0.47** / 0.66 (was 1.28 / 1.45) | **0.14** / 0.16 (0.56) | – |
| Grass near shadow map | **1.09** (1.48) | **0.36** (0.75) | – |
| AO prepass + GTAO | **1.07** / 1.18 (1.50 / 1.83) | **0.90** / 1.11 (1.08 / 1.55) | 0.37 / 0.56 (0.46 / 0.80) |
| Terrain (+ trees) | 2.59 / 3.52 (2.62 / 3.75) | 2.42 / 2.96 | 2.06 / 2.38 |
| Grass depth | **1.56** / 1.78 (1.82 / 2.19) | **1.32** / 1.51 (1.48 / 1.74) | 1.03 / 1.16 (1.10 / 1.28) |
| Grass lit | **9.23** / 10.48 (10.41 / 11.25) | **7.68** / 8.66 (8.35 / 9.26) | 5.15 / 6.03 (5.38 / 6.26) |

Grass lit with the options (wide): High 9.23 → 6.34 (cover) / 7.27 (2×) / 5.25 (both);
Medium 7.68 → 5.22 / 6.11 / 4.36; Low 5.15 → 3.50 / 4.15 / 2.93. 2× also trims terrain
(−0.2), grass depth (−0.3), sun shafts and TAA a little.

At the editor's docked Game view size (1125×558; Low at its 0.75 Render Scale, 843×418),
same harness — the editor is GPU bound, so this is its frame time:

| level | no culling | **default** | + Ground Cover | + 2× MSAA | + both |
|---|---|---|---|---|---|
| High | 14.0 (71 FPS) | **12.2 (82)** | 10.3 (97) | 10.2 (98) | 9.0 (111) |
| Medium | 10.1 (99) | **8.9 (113)** | 7.5 (134) | 7.4 (136) | 6.4 (157) |
| Low | 4.5 (222) | **4.2 (240)** | 3.4 (292) | 3.3 (302) | 2.8 (357) |

Full screen at Low's own Render Scale (1440×810): 7.8 → **7.6 ms (132 FPS)**, 6.2 with either
switch, 5.1 (196 FPS) with both.

**In the editor** (release build, MeadowDemo in Play mode, docked Game view 1125×558, VSync
off, median of 300 frames, defaults = Track A only; `perf2_editor_medium.png` shows the Stats
box): Low **4.43 ms (226 FPS)**, Medium **9.36 ms (107 FPS)**, High **12.77 ms (78 FPS)** —
round 1 ended at 212 / 98 / 71 FPS.


## Look

- **Track A**: bit-identical to `meadow-trees-1` — every one of the 12 shots (High / Medium /
  Low × wide, close, behind, oak) has 0 differing pixels (checked after each step, and again
  with everything in, Ground Cover and Anti-Aliasing at their defaults).
- **Ground Cover** against `meadow-trees-1` (`perf2_cover_<level>_<shot>.png` = before | after |
  difference × 8, `perf2_cover_medium_wide_zoom.png` = with fans / fans skipped without the
  cover / with the cover, middle distance at full size):

  | shot | no fans, no cover: mean / > 8 | **Ground Cover**: mean / > 8 / mean luma of the worst band |
  |---|---|---|
  | High wide / behind / close / oak | 0.51 / 2.6 %, 0.18 / 0.6 %, 0.32 / 1.6 %, 0.56 / 2.9 % | 0.53 / **1.8 %** / −0.4, 0.18 / 0.5 % / −0.4, 0.32 / 1.1 % / +0.3, 0.56 / 2.1 % / +0.4 |
  | Medium | 0.62 / 3.1 %, 0.21 / 0.8 %, 0.39 / 2.0 %, 0.67 / 3.5 % | 0.63 / **2.3 %** / −0.5, 0.22 / 0.6 % / −0.4, 0.38 / 1.5 % / +0.4, 0.66 / 2.7 % / +0.4 |
  | Low | 0.66 / 3.8 %, 0.26 / 1.8 %, 0.46 / 2.2 %, 0.73 / 4.2 % | 0.88 / 4.8 % / −0.6, 0.31 / 1.7 % / −0.2, 0.60 / 3.2 % / +0.2, 0.93 / 5.4 % / −0.4 |

  Without the fans the middle distance went +2.8 levels paler and bluer (pale ground specks
  between the blades); with the cover the band means are within ±0.6 and the ground reads as
  turf again. What remains is per-pixel: single specks where a fan covered or uncovered one
  sample. On Low (no TAA) the remaining speckle is a little worse than without the cover
  (4.8 % vs 3.8 % > 8 levels on the wide shot) although the band means are right.
- **2× MSAA**: see Track B2 above.

## Tried and dropped

- **Hi-Z occlusion for grass and trees**: see "Where the time was" (11 % / 7 % of lit blades
  behind the terrain, fragments already early-Z rejected; trees cost 0.2–0.8 ms in all).
- **Unordered compaction (atomics)**: fans overlap without depth writes, so a changing draw
  order changes the picture; the ordered scan keeps it bit-identical.
- **One job per (chunk, pass)**: 4.75 M record reads per frame (each pass re-tested the same
  blades), 1.0 ms of compute on High; one job per chunk with a lane per pass: 0.40 ms.
- **Ground cover as an albedo tint / analytic fan density**: see Track B1.
- **Visibility-buffer grass shading** (round 2 candidate 3): the grass study recommends it
  for after this branch (blade ids from these culled lists); not attempted here.

## For the visibility buffer

The culled lists are reusable as they are: `grass_cull.rs` `CullFrame` holds, per frame, one
compact `u32` index list (`indices()`, blade index within its chunk) and one
`DrawIndirectArgs` per (chunk, pass) (`args()`, `per_kind[kind]` maps each pass's chunks to
their argument slot); `grass_blade.wgsl` `grass_fetch(instance_index)` turns a draw instance
into its blade (group 3 = the chunk's blade buffer + the list). A visibility pass can draw
the lit lane's lists and write (chunk, instance) ids.

## Left

- Medium full screen is 15.8 ms by default, 11.7 ms (86 FPS) with both switches — not yet
  ≤ 10 ms. The lit grass is still ~4.4 ms of fragments with both on; the next step is the
  visibility buffer (shading each pixel once instead of every covering blade), or a 0.9
  Render Scale on Medium (not tried, would need the user's eye).
- 1× MSAA (Disabled) needs non-multisampled reader variants and a resolve-less main pass.
- Ground cover on Low (no TAA) leaves more speckle than on High / Medium.
- The editor's quality switch rebuilds the whole renderer (a hitch of a few hundred ms to
  seconds while caches reload); keeping GPU caches across the rebuild is possible later.

## Checks

Release profile only (disk): `cargo fmt --all`; `cargo clippy --workspace --all-targets
--release`: 0 warnings; `cargo test --workspace --release --no-fail-fast`: misty_editor 542
passed, 1 failed (the known `sample_foliage_lighting::foliage_lighting_demo_matches_its_generator`),
60 ignored; misty_engine 853 passed, 1 failed, 35 ignored; misty_player 9 passed. The engine
failure is `render::hitch_tests::scripted_meadow_run_has_no_hitches` (CPU frame-time spikes in
`Render.Culling` and `Trees.Prepare`, code this branch does not touch) while the machine ran
at 100 % CPU with ~18 other agents' rustc processes: run alone it passed once and failed
twice, and the same test on `meadow-trees-1` (a91ac1e, built in this worktree) failed too
under the same load (14 stalls vs this branch's 1–5). GPU medians in those runs were steady.
