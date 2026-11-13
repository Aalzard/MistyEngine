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

## Later rounds

- Clouds (engine in the round-1 commit): `CloudsOverride::size` (0.25..4, out of the file at
  1) and `softness` (0..1, out of the file at 0); `cloud_common.wgsl`: base tile × size, detail
  tile × √size, less erosion and a `pow` edge falloff with softness (uniform `shadow.zw`);
  Size / Softness sliders. The `git stash` "r59 clouds wip" is now applied (can be dropped).
- Plant Shadow Distance (engine in the round-1 commit): `TreeSettings::plant_shadow_distance`
  (0 = off): small plants (model radius < `SMALL_PLANT`) cast into the cascades only within it
  (CPU path `render/trees.rs`, GPU `trees_cull.wgsl` via `counts.w`).
- Cliffs, sky/haze, herbs, bark: not started.
