# NoonValley look against the references (branch r54-noon-look)

Scene content only: generator `crates/misty_editor/src/sample_noon_valley.rs`, the files it
writes under `SampleProject/Assets/NoonValley/` and `Assets/Scenes/NoonValley.scene`. Follows
`docs/pending/r26-level.md` (harness, views). Forked from main 49f7ed1b.

- Disks: target `E:/MistyTargets/r54-noon-look`, TMP `C:\MistyTry\r54tmp`, shots
  `E:/MistyTargets/r54-shots/<round>` (scratch, with local comparison sheets that hold the
  references: never commit those). Nothing on D:.
- One round = build the editor tests in a slot, `write_noon_valley`, the 9 views at High
  1920×1080 under the GPU lock (`noon_valley_shots`), `noon_valley_times` for hero and inside.
- Committed views: `docs/screenshots/r54/level_*.jpg` (ours only, 1280×720), always the latest
  round.

## Timings (High, 1920×1080, GPU / CPU render)

| round | hero | inside | notes |
|---|---|---|---|
| base (main 49f7ed1b) | 14.1 / 3.5 ms | 11.7 / 3.0 ms | machine shared with r51/r52 builds |
| e | 13.4 / 5.0 ms | 16.7 / 3.2 ms | inside: undergrowth in the open cells and hollows (+5 ms, to be cut) |
| d | – | 19.9 ms | carpets under the whole near forest: dropped |

## Rounds

- **a–e (first commit):** cliff sites: two more in the first view (250, 320) and (190, 540);
  in a site's core one continuous slab of rock (all limestone, ledges included, no litter, no
  trees except a few small ones on ledges), thin beds of 9–12 m (~55° faces over narrow
  ledges); Limestone 2048², warmer and much darker (base 0.1/0.088/0.066 linear; stains and
  lichen never paler than the rock), tile 36 m. Meadow: Meadow Ground uses the green turf (the
  far meadow was beige), grass darker and less yellow (healthy 0.44/0.53/0.22, ground tint
  0.45), taller and more uneven (0.2–0.62 m, height variation 0.9; Tall Grass 0.55–0.95 m),
  fewer, smaller flower drifts (density 28, coverage 0.13, patch 10 m); global saturation
  −10. Light: ambient gradient 0.8 and less blue (crown shade darker), far shadows 1.0, haze
  attenuation 1900 m with a greyer albedo; eye adaptation down to −3 EV. Forest: crowns
  ~20 % darker, fern and groundcover leaves darker, Forest Haze thinner (1100 m); treeline wood:
  carpets over most of the floor (never below 45 %), open cells keep their undergrowth;
  hollows in the closed wood get 8–12 plants. Clouds coverage 0.33, shape 0.6.

Numbers after e (reference / ours, sRGB means of fixed regions of the first view): meadow
near 59/76/18 / 50/66/19, meadow mid 76/87/43 / 79/89/35. Forest lower-half luminance
median: `inside` 0.019, `inside2` 0.047 (reference 0.028).

## Still differs (after e)

- Cliff slab reads pale grey and flat from the meadow, the upper part yellow-green (turf at
  the site's rim); no layering visible (beds of 9–12 m are only ~3 height samples wide).
  Sunlit terrain at albedo 0.08 already renders ~185 sRGB here, so the rock needs ~0.04.
- `inside` still shows bare sunlit litter right in front of the camera; `inside2` too pale and
  lime green next to the reference's dark olive.
- Inside GPU 16.7 ms (from 11.7): too many groundcover instances.
- Clouds: still smaller and more numerous than the reference's few big soft heaps.
