# Grass look: matching the reference meadow (branch grass-look)

Reference: `E:\MistyDesign\grass_reference.png` (not in the repo): a wide meadow of short
turf (ankle height), uneven (tufts and dips), subtle green / yellow-green patches, the ground
completely covered, soft rolling hills, soft low sunlight. The user's verdict on the grass
realism branch: "same height same color, bad shadows", too tall, ground still visible, close
shadows low quality and not sharp, no contact shadows on grass.

## Measuring

- Shots: `MISTY_GFX_STEP=<name> cargo test --release -p misty_engine grass_shots -- --ignored`
  writes `docs/screenshots/gfx/grass_<name>_{wide,close}_{golden,midday}.png` (1600×900,
  High). This branch's steps are named `look<nn>`.
- Cost and ground visibility: `grass_cost` (see grass-realism.md); numbers below are from
  one session on the RTX 4060 with the user's editor sharing the GPU (±2–3 ms between runs).
- Colour: `grass_stats.py` (scratch script, not in the repo) takes the mean and spread of
  HSV hue (degrees), saturation and value over the grass band of an image (reference: rows
  62–100 %; our wide shots: rows 60–100 %), plus the standard deviation of 24 px block means
  (large-scale patchiness) and 4 px block means (fine jitter).

### Reference (grass band)

| | hue | sat | val | patch24 | patch4 |
|---|---|---|---|---|---|
| reference | 48.4 ± 4.3 | 0.59 ± 0.08 | 0.46 ± 0.12 | 0.089 | 0.096 |

## Step log

- look00 (baseline = main at 40a612e, this session): High 34.5 ms, Medium 30.0, Low 20.7;
  ground visible 2.0 % wide golden, 0.9 % close golden. Wide midday: hue 60.7 ± 10.1, sat
  0.58 ± 0.11, val 0.38 ± 0.13, patch24 0.063, patch4 0.097 (greener and more hue-jittered
  than the reference, darker, less large-scale patchiness). Wide golden: hue 48.2 ± 7.9, val
  0.33 ± 0.17.
- look01: short turf. Heights 0.12–0.6 → 0.10–0.25 m (tufts to ~0.35 via the 5 % outliers,
  dips to ~0.06 via the area factor's floor), undergrowth 60–90 % of the local height
  (was 35–65 %), heights follow a new `details::canopy` relief (tufts and dips at 0.8 / 1.9 /
  4.5 m, the same noise as `grass_canopy` on the GPU), seed heads 10 % → 3 %, wind 0.3 →
  0.18 with gusts 0.8 → 0.55. Density 600 → 800, ground tint 1.0 and a lighter ground shade
  (0.5–0.85 of the turf's base colour), hue jitter narrowed (0.24 → 0.14), the canopy shade
  measured against the local canopy top instead of a fixed 0.45 m, the shading normal leaning
  towards the rolling canopy's normal (×5 slope), sun contact shadows on grass pixels, the
  near grass shadow map split into two slices (±4 m at 2048² = 4 mm texels, ±12 m) sampled
  with a 2×2 of half-texel taps instead of a 3×3 of whole-texel ones, Quality ▸ Grass
  Density (Low 0.5, Medium 0.75, High 1.0).
  Numbers: High 51.0 ms (51 M triangles; too many: 800/m² × 14 vertices + a second near-map
  pass), Medium 37.8, Low 22.4; ground 5.9 % wide (a red band in the fade zone 96–160 m,
  where blades shrink away, plus mid-field dots), 1.8 % close. Wide midday hue 55.8 ± 5.7,
  sat 0.64, val 0.37, patch24 0.042 (hue spread now right, still too green, too dark, too
  flat); the close blades read as fat leaves (0.022 m × the 2.6× look-down widening).
- look02: blades of four segments (10 vertices, was 14), width 0.022 → 0.013 m, density
  1000/m², LOD distance 12 m, far fade starting at 80 % of the draw distance (was 60 %), a
  broad third octave in the lush / dry patch fields (CPU and GPU), relief AO (dips of the
  canopy darker), photo detail 0.75 → 0.5 and a lower sheen, the thatch regenerated as a mat
  of dead and half-green blades (36 000 strands, soil only in cracks), lush / dry amounts
  0.8 / 0.6, seed heads 5 %. High still 50.8 ms (48 M triangles) — the cost is not the
  triangles: the lean `shade_grass` runs once per overlapping blade and a dense turf covers
  each pixel 6–14 times.
- look03: **grass depth pass**: the main pass draws the grass twice, first with
  `grass_depth` (same vertex stage, `@invariant` position, alpha to coverage, only the
  alpha written, depth Less + write) and then with the lit pipeline testing LessEqual
  without depth writes, so each covered sample is shaded once. High 50.8 → **32.6 ms**,
  Medium 24.3, Low 14.1 (baseline 34.5 / 30.0 / 20.7). Variants at High: no contact shadows
  28.7 (−4 ms), no near map 30.1, no grass shadows at all 28.2; AO Off at High 74 ms (the
  prepass still runs for the contact shadows but something else gets slow — not chased, Low
  itself is 14 ms). Ground 5.9 % wide / 1.4 % close with the original metric, which now
  counts blades that happen to match the grass-coloured ground (see look04 for the
  tint-swap measure).
- look04: width 0.013 → 0.010 m, density 1300, LOD 10 m, look-down widening 2.3×, seed
  stalks = the two panicle cells only (4 %), lit floor 0.45 → 0.55 and canopy occlusion
  floor 0.35, dip AO 0.74–1.06, lush colour darker (0.22, 0.36, 0.10), dry brighter, contact
  shadows 6 steps fading at 16 m. A second ground measure (**tint swap**: pixels that change
  by > 12 levels when only the ground's colour changes, same blades in front) joins the
  harness. High 35.1 ms (76 M triangles), Medium 25.8, Low 14.4. Shadow proof: casters
  off 29.5, cascade casters 32.0, + near map 1024² / 2048² 35.1, + 4096² **60 ms** (two
  4096² slices are fill-bound), High tier 40.8. The tint-swap numbers (58 % / 46 %) were
  wrong: the harness rendered 6 frames and a 16 m chunk at this density (330 k blades) is
  scattered one per frame, so most of the field was still empty. The patches still don't
  read: the three-octave noise sum has little contrast around the smoothstep thresholds.
- look05: harness renders 48 frames per image (every chunk arrives); the near map's inner
  slice is capped at 2048² (drawn into a viewport of its layer, uv scaled when sampling) and
  the outer slice draws a widened 60 % of the blades (one caster group per slice); patch
  thresholds sharpened (lush 0.5–0.66, dry 0.52–0.68: weights now mean 0.21 / 0.14 with a
  spread of 0.34 / 0.29 over the field); look-down widening 2.0×; density 1100, LOD 9 m,
  healthy colour (0.55, 0.58, 0.23). **High 29.5 ms** (59 M triangles), Medium 21.8, Low
  12.5. Shadow proof: casters off 25.5, cascades 27.7, + near map 29.4 (1024², 2048² and
  4096² alike now), High tier 32.3. Ground 2.0 % wide / 1.3 % close (matches-bare measure).
  The tint-swap measure still reported ~50 %: each harness render used the wall clock for
  the wind, so the blades differed between the compared images — the harness now fixes
  `grass_time` (look06). Wide midday: hue 51.6 ± 4.5, sat 0.63 ± 0.06, val 0.39 ± 0.09,
  patch24 0.044 (reference 48.4 ± 4.3 / 0.59 ± 0.08 / 0.46 ± 0.12 / 0.089): hue, hue
  spread and saturation match, still 15 % darker and far less large-scale patchiness: the
  16 m patches compress into a few pixel rows past 50 m, where most of the wide shot's
  field is.
- look06: brighter healthy / dry / lush colours (0.60, 0.63, 0.25 / 0.76, 0.68, 0.40 /
  0.26, 0.40, 0.12), patch scale 16 → 40 m (sweeps ~130 m, so the patches stay several
  rows tall deep into the wide shot), canopy relief sharpened with a smoothstep (distinct
  crests and hollows; CPU `details::canopy` and GPU `grass_canopy` alike, the GPU one with
  the matching slope). High 29.6 ms, Medium 21.7, Low 12.4 (60.6 / 41.7 / 19.9 M
  triangles). Ground: wide 3.8–4.2 % (matches bare; varies run to run with chunk arrival),
  close 1.4 %; tint swap 54 % / 37 % — the honest measure: the gaps between blades are
  the grass-coloured ground darkened by SSAO, which reads as more turf but is ground.
  Masks now per shot (`grasslook_06_{wide,close}_ground_mask.png`). Rows of the wide mask:
  the fade band at the horizon is 20 % red, but most red is spread evenly at ~3 % over the
  nearest metres (true gaps where SSAO does not reach). Wide midday hue 51.4 ± 2.8, sat
  0.61 ± 0.04, val 0.42 ± 0.08, patch24 0.042 (reference 49.2 ± 4.3 / 0.59 ± 0.08 /
  0.46 ± 0.12 / 0.087, with the scratch script of this session).
