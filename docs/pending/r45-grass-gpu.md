# Round 45: cheaper grass on the GPU, and grass LOD without pops (branch r45-grass-gpu)

Goal: the same picture for less GPU time in the grass passes. NoonValley hero at High 1920×1080
was 13.45 ms GPU against a 12 ms goal (grass visibility ~2.4 ms, shading ~2.0 ms); MeadowDemo
spends ~7–8 ms in grass. Added during the round (coordinator): every grass LOD step and the
draw-distance edge must fade, nothing may pop, cost neutral or better (r47-lod-fade does the
same for meshes, trees and impostors: tree and LOD Group code left alone).

Forked from main bf51f16b. Target `E:\MistyTargets\r45-grass-gpu`, TMP `C:\MistyTry\r45tmp`,
shots `E:\MistyTargets\r45-shots`. Scripts (scratchpad `r45/`): `times.sh` (noon hero, inside,
down at High 1920×1080 + MeadowDemo `grass_pass_times` High, per-pass medians), `shots.sh`
(the same views: noon at 1920×1080, MeadowDemo wide/close at 1600×900), `diff.py` (mean
absolute difference per image), `gdrift.py` (the golden test's measure), `lodcheck.sh` (each
LOD switch forced both ways), `summ.py`. Diagnostic shader changes go through
`MISTY_SHADER_PATCH` (test builds) without a rebuild. RTX 4060. Several timing runs were
spoiled by other GPU work on the machine (SubCraft, another branch's GPU tests); the numbers
below are from quiet runs, base and new interleaved.

## Where the time goes (hero, High 1080p, before)

Measured by switching one thing off at a time (diagnostics only, not kept):

| change (diagnostic) | Grass visibility | Grass shading |
|---|---|---|
| before | 2.37 | 1.99 |
| atlas alpha = 1 in the visibility raster | 1.17 | 1.18 |
| no lighting at all in the resolve (`grass_lit` → colour) | – | 0.71 |
| no sun shadow taps | – | 1.66 |
| no froxel fog / clouds in front (`vfog_eval`) | – | 1.77 |
| no near grass shadow map lookup | – | 1.87 |
| no screen-space AO lookup | – | 1.90 |
| no translucency term, no far sun shadow, no Misty GI, no 2nd atlas tap | – | ±0.01 |
| merge a blade's triangles within a pixel into one job | – | 1.92 (image changes) |
| no flat fans | 2.00 | – |
| grass left out of the screen-space prepass | Screen-space prepass 1.30 → 0.69 | |
| chunks drawn far to near (instead of near to far) | 2.41 | – |
| motion vectors only for the slot owning sample 0 (clock running) | – | 2.17 → 2.17 |

Resolution scan (hero, visibility): 540p 1.26, 720p 1.58, 1080p 2.37, 1440p 3.67 ms: about
0.9 ms is per blade (vertex work, ~3 M blades) and 1.5 ms per pixel at 1080p. Chunk order
does not matter, so early-z order is not the problem; the per-pixel part is mostly memory
traffic of the 4× MSAA visibility targets.

## Kept

1. **Visibility card target Rgba8Unorm** (was Rgba16Float): triangle / 255, facing 0/1,
   coverage, 4 bytes a sample instead of 8 (commit "Grass visibility: … Rgba8Unorm").
   Image: mean 0.05–0.14/255, golden measure ≤ 0.10 (coverage in 8 bits flips an
   alpha-to-coverage sample on ~0.1 % of pixels: single-pixel specks).
2. **Grass LOD fades** (no pops):
   - *Fine → coarse cards* (4 → 3 segments, per chunk at 1.5 LOD distances): the fine card's
     five rows slide along the blade to the coarse card's (0, ⅓, ⅔, tip, tip) over 80–100 %
     of that distance (`grass_coarse_morph`), so a chunk switching changes nothing. Forced
     switch at 1.5 LOD vs never (hero): before mean 0.121/255, 0.31 % of pixels > 8; after
     0.003/255, 0.00 %.
   - *Draw-distance edge*: every blade's width × the distance fade (it already shrank in
     height over 80–100 % of the draw distance; the flat fans kept their full width past it to
     the end of the last chunk), and the culling leaves out everything past the draw
     distance. Chunks starting past it dropped vs kept: no change (before 0.001/255 too in
     the hero: the edge sits under the far haze there).
   - *Screen-space prepass range* (20 m on High): its blades thin out whole, a random share
     by phase, over 75–100 % of the range instead of ending at whole chunks
     (`grass_prepass_out`).
   - The leaf count across a card (`repeat`) is taken from the card's full width, so the
     fade narrows leaves without re-cutting them.
   - Test knobs `MISTY_GRASS_COARSE_FROM`, `MISTY_GRASS_DROP_BEYOND` (test builds) and the
     `noon_valley_dolly` harness.

## Tried and dropped

- **Composite per pixel with a sample mask** (one fullscreen triangle per slot): 0.43 →
  1.22 ms. Reverted.
- **Two-segment cards past 10 LOD distances, one-segment far fans**: no measurable gain
  (triangle counts in the stats unchanged), and each is another chunk-wide switch. Reverted.
- **Motion vectors only for sample 0's slot**: no gain (every warp has a sample-0 slot).
- **Smooth leaf count** (the next leaf growing in instead of `round`): removes a per-tuft
  re-cut while walking, but changes the turf everywhere (golden measure 0.6–1.8 on close /
  down shots, over the 0.5 limit even with a 6 % band). Not kept; the re-cut is per tuft
  (each at its own distance), not a ring.
- Lazy fog light (`vfog_analytic_light` only where used): timings spoiled by other GPU load,
  not concluded.

## Results (High, 1920×1080, base bf51f16b vs 8ea3aa74, 3 interleaved runs, medians)

| pass (ms) | hero before | hero after | down before | down after | inside before | inside after | MeadowDemo before | MeadowDemo after |
|---|---|---|---|---|---|---|---|---|
| Grass culling | 0.22 | 0.22 | 0.15 | 0.15 | – | – | 0.42 | 0.42 |
| Grass near shadow map (inner + outer) | 0.69 | 0.69 | 0.71 | 0.71 | – | – | 0.88 | 0.88 |
| Screen-space prepass | 1.31 | 1.13 | 0.80 | 0.78 | 0.75 | 0.75 | 0.51 | 0.35 |
| Grass visibility | 2.37 | 2.07 | 2.54 | 1.71 | – | – | 2.51 | 2.17 |
| Grass shading | 2.00 | 1.89 | 2.23 | 2.14 | 0.15 | 0.14 | 1.67 | 1.58 |
| Main: grass composite | 0.43 | 0.42 | 0.75 | 0.74 | 0.06 | 0.06 | 0.47 | 0.47 |
| Main: terrain | 0.95 | 0.93 | 1.02 | 1.03 | 1.33 | 1.32 | 2.00 | 1.87 |
| Sum of all passes | 13.48 | 12.85 | 12.89 | 11.94 | 11.94 | 11.73 | 14.37 | 13.59 |

(The frame totals of these runs are noisier than the pass sums: another branch's GPU tests ran
on the machine part of the time; the quiet runs earlier gave hero 13.40 → 13.20 for step 1
alone.)

Images (same views, settled 64 frames; noon 1920×1080, MeadowDemo 1600×900):

| view | mean abs diff /255 | pixels > 8 | golden measure (5× down, limit 0.5) |
|---|---|---|---|
| hero | 1.21 | 5.8 % | 0.43 |
| down | 0.44 | 1.5 % | 0.22 |
| inside | 0.19 | 0 % | 0.19 (run-to-run 0.08) |
| MeadowDemo wide | 0.61 | 2.0 % | 0.20 |
| MeadowDemo close | 0.50 | 1.9 % | 0.18 |

Most of the hero's difference is the band 4.8–6 m where the fine cards now take the coarse
card's shape (the price of no pop at the switch); step 1 alone is 0.07/255.

LOD switches forced both ways (hero, High 1080p): fine→coarse at 1.5 LOD distances vs never:
before 0.121/255 (0.31 % of pixels > 8, a band across the meadow at 6 m), after 0.003/255
(0 %); chunks past the draw distance dropped vs kept: 0.001/255 before and after. Camera
dolly (`noon_valley_dolly`, 24 frames × 0.1 m from the hero spot): no step with a block that
changes more than 1.5× the run's typical step, before or after (the near grass's own motion
and TAA dominate frame-to-frame differences; the forced-switch check is the sharper test).
Sheets (not committed): `E:\MistyTargets\r45-shots\before_after_sheet.jpg` (before / after /
45-shotsefore_after_sheet.jpg` (before / after /
difference × 8) and `lod_switch_sheet.jpg` (forced-switch differences × 8, before left).

## Not done

- The near grass shadow map (0.7 ms) and the grass in the screen-space prepass are the next
  costs; merging the grass into the prepass would need the visibility raster before the
  ambient occlusion (a reordering of the main pass), not tried.
- Per-pixel overdraw of the visibility raster (1.5 ms of it at 1080p) is mostly the cards'
  alpha; nothing found that keeps the look.
- The leaf count across a card still steps per tuft as its width passes a half leaf (see
  above); fans that enter a chunk's drawn prefix still appear whole (both per blade, not
  rings).

## Paused 2026-11-12 18:25 (user pause): where this stands

- Committed: ebda8d5c (Rgba8Unorm card target), 8ea3aa74 (LOD fades), af25e9b7 (notes). The
  tree is clean; nothing of r45 holds a build slot or the GPU lock.
- Checks on 8ea3aa74: `cargo fmt --all --check` passes; clippy `--workspace --all-targets
  --release`: 0 warnings; workspace tests: editor 918 passed, engine 1521 passed / 1 failed
  (`render::hitch_tests::scripted_meadow_run_has_no_hitches`: one 95 ms wall-clock frame with
  3.5 ms CPU and 9.6 ms GPU while the machine was 48 % busy with other jobs; not rerun alone
  yet), every other crate passes.
- **Golden shots: 19 of 20 pass, `meadow_low_close` drifts: mean 0.55 (limit 0.5)**. All
  MeadowDemo shots moved 0.28–0.55 (Low the most: wide 0.47, behind 0.45, oak 0.42); cycles
  0.00. Do not update the golden images. Was finding which fade causes it when paused: a
  script (`scratchpad r45/gold.sh`) runs golden_shots with base.exe (base drift) and with
  final.exe + the patches `nomorph`, `noprep`, `nofade` (scratchpad `r45/patches/`); it had
  not started (waiting for the GPU lock).
- **Next:** 1) run that attribution; likely fixes: start the coarse-card morph later (band
  0.9–1.0 instead of 0.8–1.0) and/or keep the prepass thinning only where the prepass range
  is ≥ 12 m (Low and Medium's 8 m range puts the thinning at 6–8 m, close to the camera); 2)
  rebuild, rerun golden_shots under the lock (must pass), the hitch test alone, and the
  forced-switch check (`lodcheck.sh`); 3) refresh the tables above and the Round 45 section of
  docs/progress.md if numbers change.
