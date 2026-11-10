# NoonValley level work (branch r26-level)

Scene content only (generator `crates/misty_editor/src/sample_noon_valley.rs`,
`tools/noon_valley_textures.py`, `SampleProject/Assets/NoonValley/`, `Assets/Scenes/NoonValley.scene`).
Engine code stays on `r26-noon-meadow` (merge it in now and then: `git merge r26-noon-meadow`).
Forked from `r26-noon-meadow` at 80e185bb.

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

## Timings

(none recorded yet; r9's `times.txt` is the first)

## Engine requests for the engine agent

(none yet)
