# NoonValley level work (branch r26-level)

Scene content only (generator `crates/misty_editor/src/sample_noon_valley.rs`,
`tools/noon_valley_textures.py`, `SampleProject/Assets/NoonValley/`, `Assets/Scenes/NoonValley.scene`).
Engine code stays on `r26-noon-meadow` (merge it in now and then: `git merge r26-noon-meadow`).
Forked from `r26-noon-meadow` at 80e185bb.

## Progress log (second agent, from 2026-11-10 06:00)

- Merged `r26-noon-meadow` (3a254653 = the regeneration from 80e185bb's generator), so the
  committed scene is r9's baseline; r9 rendered from it without regenerating.
- r10 generator edits committed as WIP (not yet regenerated; `noon_valley_matches_its_generator`
  fails until `write_noon_valley` runs): hill foot 90 − 0.2·p.y, hill 600·(1 − e^(−0.85·soft/600))
  with ridge 1.0 + 0.2·fbm, SIZE.y 700 → 1000 (room for the taller hill), forest edge
  smooth(−5, 12, u + 18·fbm), no clearings, forest to u 900, canopy gaps only where
  hill_side < −10..−40, tree slope limit 47°, Hill Turf [22, 48], Limestone [48, 90], a second
  Forest Edge tree per edge cell within 1.2 km, Forest Edge 16 m / crown 0.9, litter only where
  forest > 0.75, limestone → turf beyond far_radius 1100–1300, near-range terraces removed,
  Tall Grass 12/m² [0.4, 0.7] dryness 0.35 tint 0.15, flowers patch 16 / coverage 0.28 / LOD 55.
- `tools/noon_valley_textures.py`: fern and groundcover atlases 1024 → 2048 (line widths and blur
  scaled by `AK`); generated into `D:/MistyTargets/r26-level-scratch/tex2k/`, to be copied into
  `Assets/NoonValley/Textures/` with r10 (after r9's render).
- Build rule from the coordinator: every cargo run inside `source /d/MistyTargets/build-slot.sh;
  take_slot r26-level; cargo ... -j 4; drop_slot`. `round.sh` now builds in a slot and runs the
  test executables directly (`SKIP_BUILD=1` reuses `engine_build.log`'s executable).

- r9 (baseline, shots `scratchpad/r26/shots/r9/`): hero still the far, low hill; forest edge
  of separate domes with litter showing; inside views now have trunks and a few sun flecks but
  the floor is nearly black, the canopy closed (every 8 m cell a 13 m wide crown), bushes and
  groundcover flat bright green. Clouds: one big heap overhead, none mid-sky in the hero.
- r10 adds for the interior (treeline wood only, `near_wood`): a third of the trees dropped,
  gap threshold 0.66 − 0.14·near_wood; Forest Floor occlusion 0.6 → 0.4; eye adaptation
  limit_min −3.2 → −4.5 EV (inside2 was clamped dark). Clouds coverage 0.42, altitude 1300,
  thickness 1000 (smaller heaps, more of them lower in the sky).

- r10 (regenerated, 116 372 trees; committed 41a5ea5e): hero has the steep forested wall on
  the right; inside2 bright with trunks apart and sun patches (mean RGB ≈ the reference's);
  inside still black in its lower half (shaded floor ~200× darker than sun, the reference ~10×),
  groundcover rendered at twice its size (the loader clamps recipes to ≥ 1 m / 2 cm; fixed in
  e763c372 by 1 m recipes scaled per instance). Coordinator: salmon bare patches on the hillside,
  a grey-white frost-like flower patch mid-meadow, bare ground and trunks under the edge crowns,
  pale flat sky.
- r11 generator: `wood_depth` (least forest cover on a 20 m ring) decides the edge: low Forest
  Edge crowns and bushes (plus the second edge tree) over the outer ~20 m, litter only behind it
  (smooth(0.55, 0.95, depth)) and never on slopes > 44–50°; treeline wood keeps half its trees,
  gap threshold 0.66 − 0.18·near_wood; Forest Floor occlusion 0.15; groundcover leaf colour
  0.6/0.66/0.54 and bush 0.66/0.72/0.6 with crown shading 0.9; flowers coverage 0.22, patch 12,
  LOD 45, daisy and buttercup 1:1; sky tint 0.30/0.36/0.46; limestone albedo greyer.

- r11 (regenerated, 116 222 trees; c8588174; shots in `docs/screenshots/r26/level_r11_*.jpg`):
  hero and forest views now have the steep wall of crowns with low crowns to the grass, no pink
  litter, deeper sky; flowers in small drifts. Still off: cumulus overhead are flat sheets, not
  separate heaps; inside/inside2 shade on the floor is black next to blown-out sun patches (the
  thinner canopy shows more sky, so eye adaptation exposes for it). Recipe crown_width is also
  clamped (≤ 2.0): Groundcover set to 2.0.
- r12 generator: eye adaptation `histogram_high` 80, compensation +0.3 EV; environment
  lighting 0.75 → 0.9; Forest Floor occlusion 0; groundcover translucency 0.15; clouds coverage
  0.34, shape 0.7, altitude 1500, thickness 1400.

- r12 (c19a068e): meadow brighter and closer to the reference's greens, cumulus heaps overhead;
  the forest floor's shade went pitch black. Cause: `TerrainLayer::occlusion` is the sky light
  that *reaches* the layer (1 = open ground), so r11's 0.15 and r12's 0 removed it (r10's 0.4
  was already dim). Fixed to 0.9 (eb272f96).
- r13 generator: occlusion 0.9; ferns and broad-leaved plants (2–4 per cell, 65 % ferns) in the
  treeline wood's dropped cells and canopy gaps; a quarter birches there.

- r13 (32588f6d; `docs/screenshots/r26/level_r13_*.jpg`): the forest floor is visible and
  mid-toned in both inside views, birches and ferns between spaced trunks. Now too even: the
  shade is nearly as bright as the sun flecks, the litter texture reads coarse, the undergrowth
  sparse next to the reference's carpet of ferns and leaves.
- Merged `r26-noon-meadow` 24abc5b6 (final engine work: understory no longer glows under a
  canopy, occlusion culling off by default, docs). Clippy fixes in the generator (fcc0a28b).
- r14 generator: floor sky light 0.65, litter tile 3 → 2 m, compensation +0.15 EV, gap cells 3–6
  plants at 0.9–1.7× scale, one more plant per tree in the treeline wood, grass lod_distance
  6.5 → 5.8 (≈ 20 % fewer distant blades, for the grass cost).

## State (handed over 2026-11-10, no scene changes committed yet)

- Round r9 = the baseline of 80e185bb (Tree Clumping, small-plant LOD and shadows, Terrain Tree
  Shadow Detail, high crowns, canopy gaps, clear inside views). 80e185bb changed the generator but
  did **not** regenerate `NoonValley.terrain` / `.scene` (its diffstat has no .terrain/.scene), so
  `noon_valley_matches_its_generator` fails on disk until `write_noon_valley` is run. r9's first
  step is that regeneration.
- Build: a full release build of the editor test binary (`misty` bin tests) and the engine lib
  tests into `D:/MistyTargets/r26-level` was running in the background (log
  `D:/MistyTargets/r26-level-scratch/build0.log`, prints `== done` when finished). If the target
  directory is complete, later rounds only rebuild `misty_editor` (a few minutes).
- Scratch scripts (D: only, nothing on C:):
  - `D:/MistyTargets/r26-level-scratch/env.sh`: CARGO_TARGET_DIR, TMP/TEMP, `take_lock` /
    `drop_lock` for the GPU lock `MistyWork/gpu-timing.lock` (mkdir + owner file "r26-level").
  - `D:/MistyTargets/r26-level-scratch/round.sh <name> [views] [quality] [size] [regen=1] [time=1]`:
    regenerates the sample, renders the 9 harness views into the scratchpad
    `r26/shots/<name>/`, runs `noon_valley_times` for hero,inside at High 1080p (writes
    `times.txt`) and builds `sheet_meadow.jpg` / `sheet_forest.jpg` with `r26/tools/sheet2.py`.
  - References and r0–r8 shots: scratchpad `r26/` (`reference.webp`, `reference_forest.webp`,
    `shots/r8/sheet_meadow.jpg`, `shots/r8/sheet_forest.jpg`). Never commit the references.
- Performance mandate (from the user): every round or two run `noon_valley_times` for hero and
  inside on High at 1080p under the GPU lock and note GPU / CPU ms here; aim for GPU < ~12 ms and
  CPU < ~8 ms on High by densities, LOD / impostor settings, texture sizes and draw distances;
  where only engine work can meet it, write the request with numbers here.

## What r8 shows against the references (the differences to fix)

Meadow reference vs r8 hero / forest / treeline views:
1. The forested hillside is too far, too low and too gentle: in the reference a steep wall of
   packed crowns enters the frame ~100 m to the right and fills it to the top; ours is a shallow
   distant hill whose ridge barely clears the horizon even in the `forest` view (pitch 14).
2. The forest is not closed: bright grassy clearings and bare brown litter show between separate
   dome crowns along the edge; the reference has foliage to the meadow edge, no ground visible.
3. Too many tall pale seed-head spikes in the grass (the "Tall Grass" detail layer: dryness 0.6,
   tint 0.3, 35/m², 0.45–0.85 m).
4. Flower drifts: the reference has pale cream bands visible 100–300 m out; ours only yellow specks
   near the camera (flowers are grass-system cards, drawn to the grass draw distance 160 m with
   keep = (lod_distance / dist)²; lod_distance 30).
5. The far ranges show white limestone patches like snow (limestone rule slope 46–90 applies
   everywhere, plus the near range's terraces); the reference ranges are blue-hazed forest with
   grey rock peaks.
6. The sky is a pale grey-blue with no clouds in the hero view; the reference is deep blue with
   5–6 puffy cumulus mid-elevation. (Engine sky model: `sky_color` in `shaders/common.wgsl` mixes
   horizon→zenith by pow(dir.y, 0.6); `sky_tint` scales the zenith by tint × thin², horizon by
   half the tint.)
7. The lone Meadow Oak renders nearly black from the `forest` view (crown_shading 0.7 + AO).

Forest reference vs r8 inside / inside2: nearly black (camera inside a bush, no sun patches,
closed canopy, flat bright groundcover cards). 80e185bb addresses this but is unrendered: judge r9
first.

## Next three planned changes (round r10, batch them into one regeneration)

1. **Hillside closer and taller** in `hill_side` / `height_m`: foot ≈ 90 − 0.2·p.y (was
   150 − 0.16·p.y) so the forest edge enters the hero frame ~115 m ahead; hill ≈
   R·(1 − exp(−k·soft/R)) with R ≈ 600, k ≈ 0.85 (≈40° at the foot, ridge factor 1.0 + 0.2·fbm);
   raise the tree slope limit from 42° to ~47° and the limestone rule to [48, 90], Hill Turf to
   [22, 48], so steep forested slopes are not bare. Keep the lone oak at (46, 34) in the meadow.
2. **Close the forest**: drop the clearings (or ×0.3), narrow the ragged edge band
   (smooth(−5, 12, edge)), plant a second edge tree per cell so the edge is a wall of low crowns
   (Forest Edge height 16, crown_width 0.9), litter layer only where forest > 0.75
   (smooth(0.75, 0.98)), and restrict the canopy gaps (`near_wood`) to the flat treeline wood
   (`hill_side` < −30) where the inside views are, not the hillside seen from the meadow.
3. **Grass, flowers, far ranges**: Tall Grass density 35 → ~12, dryness 0.6 → 0.35, tint 0.3 →
   0.15, height [0.4, 0.7]; flowers patch_scale 9 → 16, coverage 0.2 → 0.28, lod_distance 30 →
   55 (check the cost in times.txt); in the alphamap loop convert limestone weight to turf beyond
   `far_radius` > 1200 m, and set the near range's terrace `rocky` to 0.

After r10: sky (deeper `sky_tint`, cumulus coverage ~0.45 or a wind offset so clouds sit in the
hero view), the lone oak's darkness, then the forest interior against `reference_forest.webp`.

## Timings (High, 1920×1080, `noon_valley_times`; machine heavily loaded by other agents)

| round | hero GPU / CPU | inside GPU / CPU | notes |
|---|---|---|---|
| r13 | 20.4 / 2.5 ms | 14.2 / 2.3 ms | |
| r12 | 20.4 / 3.5 ms | 14.2 / 3.4 ms | |
| r11 | 20.4 / 4.0 ms | 14.3 / 3.6 ms | |
| r10 | 20.4 / 3.9 ms | 14.6 / 3.7 ms | trees 3.5 (hero) / 2.3 (inside, half the wood's trees) |
| r9 | 20.1 / 3.8 ms | 15.8 / 3.6 ms | hero: grass ≈ 8 ms (visibility 3.4, shading 2.3, near shadows 1.6, culling 0.5, composite 0.4), trees 3.1, sun shafts 0.9; inside: shadow maps 2.6, shadow mask 2.3, trees 3.3 |

## Engine requests for the engine agent

1. `TreeAsset::sanitize` clamps height to ≥ 1 m and trunk radius to ≥ 2 cm, so a 0.5 m
   groundcover or 0.9 m fern recipe silently loads at 1 m (worked around with per-instance
   scale). Please allow ~0.2 m / 5 mm for small plants.
2. Light under tree canopies: on High the shaded forest floor is ~200× darker than the sunlit
   patches (reference ~10×), so `inside` is black below the horizon even with eye adaptation at
   −4.5 EV and Forest Floor occlusion 0.15. Sky/ambient visibility under trees (GTAO + Misty GI +
   crown shading) looks too strong; a floor of indirect light bounced from the canopy and the
   sunlit floor would fix it.
3. Grass cost: hero at High 1080p is ~20 ms GPU, grass ≈ 8 ms of it (visibility 3.5, shading 2.0,
   near shadow maps 1.5, culling 0.5, composite 0.4), trees 3.5, sun shafts 0.9. The 12 ms target
   needs cheaper grass passes; scene-side cuts (density 800 → 600, lod_distance 6.5 → 5.5) would
   thin the meadow visibly.
4. Harness: the `treeline` view (offset 40, 200) now stands at the hill's foot (foot ≈ 50 m there);
   (−20, 180) would keep it in the meadow looking along the edge.
5. Clouds: no seed or offset to slide the cumulus field, so the hero view's sky can't be composed
   (the reference has 5–6 mid-elevation heaps in frame).
