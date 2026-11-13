# NoonValley round 59 (branch r59-noon2) — WIP, paused

Disks: target `E:/MistyTargets/r59-noon2`, TMP `C:\MistyTry\r59tmp`, shots
`E:/MistyTargets/r59-shots/<run>` (scratch). Scripts in the session scratchpad `r59/`
(`env.sh`, `fade.sh`/`fade.py` grass on/off measurement, `both.sh` build, `regen.sh`).

## Round 1 (top priority): grass-to-ground transition — in progress, not yet verified

Engine (default off, so MeadowDemo/golden unchanged; not yet golden-checked):
- `DetailSettings::far_field` (0..1, out of the file at 0) and `far_field_brightness`
  (0.2..2, out of the file at 1); passed in `TerrainParams.chunks.yz`.
- `terrain_layers.wgsl`: where grass grows, from 4–24 m on, the ground takes the grass's
  colour and patches × brightness × a view term (blades glow looking towards the sun, darker
  with it behind: `1 + 0.94·dot(view.xz, to_light.xz)`), blade-scale detail from the ground
  texture relative to its mean, a tufted normal (two noise octaves, the fine one faded by pixel
  footprint) and canopy AO (`far_ao`). The plain ground-tint path is unchanged when far_field=0
  (its near darkening `under` now only applies with a ground tint).
- Editor: Grass & Details ▸ Far Field / Far Field Brightness sliders (tr/ar strings added).
- Test knobs: `MISTY_NOON_GRASS=farfield:..,farbright:..,tint:..`.

Scene: `meadow_turf()` rewritten (2048², ~110k short blades in every direction, lighter tips,
yellow / deep-green clumps, straw blades, height map from the blades); `far_field: 1.0`,
`far_field_brightness: 0.46`. NOT regenerated yet (the scene files on disk are the old ones).

Measurement (`fade.sh`: grass on vs `MISTY_GRASS_DROP_BEYOND=-1`, same terrain shading, eye
adaptation off; "grass / terrain under it" = luminance of the grass-on band just inside the
grass's far edge over the same pixels with grass off, 1.00 = no step):
- before (main): visible pale smooth band past the blades (grazing view); hero step 1.25×.
- far field 1.0, brightness 0.6 (old turf, no view term): pose 0.90, hero 0.85.
- brightness 0.47: pose 0.99, hero 0.90 → the view term above was fitted from these two
  (blades vs far field per band: hero 0.067/0.091, pose 0.099/0.106).

Next: finish the build (`both.sh`), `regen.sh`, `fade.sh r1`, check both views ≈1.0 and the
grazing shot by eye, adjust brightness, then checks + golden, commit round 1 with before/after
shots (`docs/screenshots/r59/fade_*.jpg`, ours only) and a Round 59 section in progress.md.

## Later rounds (not started in the repo)

- Clouds: `CloudsOverride::size` (0.25..4, out of the file at 1) and `softness` (0..1, out of
  the file at 0), shader `cloud_common.wgsl` (base tile × size, detail tile × √size, less
  erosion and `pow` edge falloff with softness), Size / Softness sliders (Softness tr/ar
  added). Kept aside in `git stash` "r59 clouds wip" (also saved as `clouds-wip.patch` in the
  scratchpad); NoonValley would use size 2.4, softness 0.5, coverage 0.34, thickness 1500.
- Forest perf idea: a per-terrain plant shadow distance for small plants (radius <
  `SMALL_PLANT`), CPU path in `render/trees.rs` and GPU `trees_cull.wgsl` (`counts.w` free).
- Cliffs, sky/haze, herbs, bark: not started.
