# NoonValley round 59 (branch r59-noon2)

Disks: target `E:/MistyTargets/r59-noon2`, TMP `C:\MistyTry\r59tmp`, shots
`E:/MistyTargets/r59-shots/<run>` (scratch). Scripts in the session scratchpad `r59/`
(`env.sh`, `fade.sh`/`fade.py` grass on/off measurement, `both.sh` build, `regen.sh`).

## Round 1: grass-to-ground transition (done)

Engine (default off, so MeadowDemo / golden unchanged):
- `DetailSettings::far_field` (0..1, out of the file at 0) and `far_field_brightness`
  (0.2..2, out of the file at 1), passed in `TerrainParams.chunks.yz`.
- `terrain_layers.wgsl`: where grass grows, weight `far_field × cover × smoothstep(4, 24, dist)`:
  albedo = grass colour with its patches and swathes × brightness × tuft mottle × the ground
  texture's detail relative to its own mean (mip 12); normal = upright (mix of the terrain
  normal and +Y, 0.6) tilted by two noise octaves (the 0.8 m one faded by pixel footprint);
  ambient occlusion `far_ao`. A view term (blades glowing towards the sun) was tried and
  dropped: the needed brightness did not follow the sun direction but the slope (sun-facing
  slopes lit the flat ground more than the blades), hence the upright normal.
- Editor: Grass & Details ▸ Far Field, Far Field Brightness (tr / ar).
- Test knobs: `MISTY_NOON_GRASS=farfield:..,farbright:..,tint:..,plantshadow:..`.
- Scene: `meadow_turf()` 2048² blades; `far_field 1.0`, `far_field_brightness 0.3`.

Measurement (`fade.sh` in the scratchpad: each view rendered with the grass and with
`MISTY_GRASS_DROP_BEYOND=-1`, same terrain shading, `MISTY_NOON_AE=off`; the grass's far edge
found as the topmost rows where most ground pixels change; ratio = grass-on luminance over
grass-off on the same pixels, 16–60 rows inside the edge, flowers masked out):

| run | brightness | measuring pose | hero | away | left |
|---|---|---|---|---|---|
| main (far field off) | – | visible pale band; hero step 1.25× | | | |
| e1 (old turf) | 0.6 | 0.90 | 0.85 | | |
| e2 (old turf) | 0.47 | 0.99 | 0.90 | | |
| r1 (new turf, view term 0.94) | 0.46 | 0.94 | 0.86 | | |
| r2 (view term 1.74) | 0.40 | 1.02 | 1.08 | 0.67 | 0.86 |
| r3a (upright normal, no view term) | 0.36 | 1.07 | 0.90 | 0.82 | 0.98 |
| r3b | 0.28 | 0.93 | 1.08 | 0.89 | 1.16 |
| r4 (committed) | 0.30 | 0.91 | 1.03 | 0.92 | 1.11 |

Still differs: ±10 % per view (the blades' brightness depends on the slope and sun in ways a
single brightness does not follow); at eye level the far meadow is still a little lighter than
the grass mass (more haze and the sun-facing far slopes), and where the grass ends on a crest
the line follows the crest.

Checks for round 1 (tree with the cloud and plant-shadow engine controls too): fmt clean,
clippy `--workspace --all-targets --release` 0 warnings, `cargo test --workspace --release
--no-fail-fast` 936 editor + 1588 engine + 11 + 3 passed, 0 failed; golden 20 / 20 ok.

## Round 2: eye-level grass edge, clouds, sky, forest and cliff first pass

Engine: wider blade fade with the far field on (`terrain_grass.rs`: fade from
`far × (0.8 − 0.25 × far_field)`); far field darkens past the blades
(`terrain.rs FAR_SHADE` 0.35, `MISTY_FAR_SHADE` in tests; shader `past` = smoothstep(0.55,
1.6, dist / draw)), × (0.92, 1, 1) colour, upright 0.85.

Eye-level grazing view (`fade.sh` graze, eye adaptation on, sRGB, rows 560–585 far meadow,
600–615 just past the crest, 630–660 grass mass, columns 300–1000):

| run | far | past crest | grass |
|---|---|---|---|
| main | 107/119/87 | 82/91/48 | 57/66/27 |
| round 1 (r4) | 101/109/74 | 78/84/41 | 57/66/24 |
| far shade 0.25 (s1/s2 0.5) | – | – | – |
| far shade 0.35 (s3) | 83/93/68 | 70/78/40 | 56/65/24 |
| far shade 0.5 (s4) | 76/85/66 | 69/77/40 | 56/65/24 |
| round 2 (r5, with the darker sky: higher exposure) | 71/78/55 | 78/84/43 | 77/87/35 |

Measured on/off (16–60 rows inside the edge) r5: measuring pose 1.17, hero 1.07, away 0.96,
left 1.13. The "trend" metric (grass-side row trend carried past the edge) is too noisy on
rolling ground to use (0.74–1.50).

Timings r5 (High 1920×1080, after main's round 57 perf merge): hero GPU 11.91 ms (parts
11.49), CPU 4.15; inside GPU 14.32 ms (parts 13.76; shadow maps 2.88, trees 2.07, grass
0.44 + 0.72), CPU 3.04. r54 end: hero 13.5, inside 19.4 (parts 14.7, shadow maps 3.53).

Still differs after round 2: far meadow greyer than the grass mass just before the crest;
meadow brighter than the photo (the darker sky raised the eye adaptation); herbs still read as
pale sprouts; `inside2` crowns still light; inside 1.6 ms over target; cliffs still smooth
from the meadow (no rock meshes yet).

## Round 3: grey band, inside cost, outcrops

Eye level (sRGB, same bands): r5 far 71/78/55, past crest 78/84/43, grass 77/87/35;
r6 / r7 far 58/65/43, past crest 65/71/32, grass 64/74/25. On / off at the edge (16–60 rows):
1.17 / 1.07 / 0.95 / 1.12 (pose, hero, away, left).

Timing tries with the test knobs (inside, High 1920×1080): as regenerated in r6 15.3 ms
(parts 14.3, shadow maps 2.8); `shadowdetail:0.2,plantshadow:4` 14.0 (12.8, shadow 1.6);
plus `treelod:20` 12.8 (12.4). Committed (r7): inside 13.42 ms GPU, parts 12.33, CPU 2.9;
hero 12.31, parts 11.88. Herbs are still 0.6 + 0.8 ms (the next cut if the inside view must
drop further).

Still differs: the far meadow (200–500 m) is darker grey-olive than the photo's lighter
olive; herbs still read as pale sprouts in the sun; the cliff terrain patch on the left of the
first view is a beige patch with tree dots (no outcrops show there); `inside2` crowns lighter
than the photo; inside GPU ~0.7 ms over 12.7.

## Round 4: far meadow, herbs, crowns, outcrops

- `FAR_SHADE` 0.15 and the far field's shade easing off from 1.8× to 4.5× the grass distance
  (×(1 − 0.6·smoothstep)). Eye level sRGB far / past crest / grass: r7 58/65/43, 65/71/32,
  64/74/25; test knob 0.15 (f15) and r9 65/73/44, 66/73/33, 64/74/25.
- Forest Herbs layer removed (function deleted). Inside 10.78 ms GPU (parts 10.60, 10.2 M
  triangles, 422 draws), hero 12.65 (12.24).
- Wood crowns [0.44–0.47, 0.49–0.52, 0.31–0.33]: `inside2` lower-half median 0.024 (photo
  0.028).
- Outcrops in the sites (150, 800), (280, 980), (40, 1020), 14–24 m; slabs over 6 m tilt 0.85
  to the slope and sink 0.25 of their footprint.
- Not done: moss / staining at the outcrop feet (shared rock materials from `sample_rocks`;
  would need NoonValley material variants and prefab overrides, or terrain paint around each
  outcrop computed in `terrain_data`).

## Later rounds

- Clouds (engine in the round-1 commit): `CloudsOverride::size` (0.25..4, out of the file at
  1) and `softness` (0..1, out of the file at 0); `cloud_common.wgsl`: base tile × size, detail
  tile × √size, less erosion and a `pow` edge falloff with softness (uniform `shadow.zw`);
  Size / Softness sliders. The `git stash` "r59 clouds wip" is now applied (can be dropped).
- Plant Shadow Distance (engine in the round-1 commit): `TreeSettings::plant_shadow_distance`
  (0 = off): small plants (model radius < `SMALL_PLANT`) cast into the cascades only within it
  (CPU path `render/trees.rs`, GPU `trees_cull.wgsl` via `counts.w`).
- Cliffs, sky/haze, herbs, bark: not started.
