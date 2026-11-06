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
- look07 (lead feedback on look06: olive / mustard colour, upright sedge-like straps, weak
  large-scale patchiness, dead-flat field, ground 2 % / 1.3 %):
  - **Rolling hills**: MeadowDemo's height gets `0.02 × fbm(90 m, 2 octaves)` (soft swells
    of a few metres over 60–120 m); the camera still stands 1.6 m above the ground.
  - **Turf**: 0.07–0.20 m (was 0.08–0.22), blades 8 mm, bend 0.62 → 0.8 (more lean), every
    blade's own height weighs 40 % (was 25 %) so the top line breaks up, undergrowth lower
    (0.6–0.9 of the local height) and 4× wider; tip dryness 0.25 → 0.15.
  - **Multi-leaf cards**: a card wider than its specimen (undergrowth, the look-down
    widening, LOD widening) now shows `round(width / specimen width)` narrow leaves side by
    side (≤ 4, each ≥ 2.5 px), the first at full length and the others 65–100 % of it, so
    wide cards read as ragged tufts of fine leaves instead of fat straps. The card
    coordinate carries the cell (8 × cell + 0..repeat) and a random blade number (v + 2 ×
    n), shared by the lit, depth, prepass and shadow passes (`grass_card_u`,
    `grass_card_v`, `grass_atlas_uv`); look-down widening 2.0 → 1.75×.
  - **Ground coverage at the same blade count**: LOD widening was `1 / sqrt(keep)`, which
    let the blade area per m² fall as 1 / distance; it is now `1 / keep` (area preserved;
    `MAX_WIDEN` 4 → 8), LOD distance 9 → 7.5 m. Tint-swap ground 54 % / 37 % → 33 % / 26 %
    in the intermediate run before the leaf variation; the ragged leaves give some back.
  - **Colour**: per-blade dryness mostly green (`(dry − 0.45) × 1.6`), the golden share
    only in the dry patches; broad **swathes** (`grass_swathe`, 1.5 patch sizes ≈ 60 m,
    ±20 % value, warmer when bright; CPU `details::swathe` makes bright swathes a little
    shorter) on blades and ground; lush patches toned down (0.24, 0.34, 0.10 at 0.6);
    healthy (0.56, 0.65, 0.22); canopy floor brighter (lit 0.55 → 0.7, occlusion 0.35 →
    0.5, dips 0.74 → 0.84) so the near field is not darker than the reference; far blades
    and the ground under them take a warm far tint (tips and seed stalks, `grass_far_tint`
    6–60 m) and the canopy-normal lean is capped at 0.6 (it lit the far field with the
    whole blue sky: lime). The ground stand-in far away is darker (0.85 → 0.66 of the
    base) and mottled at 1.3 / 6 / 21 m from 8 m out; blade detail and colour jitter now
    fade over 5–55 m (was 4–35 m), so the field keeps texture to ~60 m.
  - **Sun**: 26° → 30° high, colour (1, 0.86, 0.66) → (1, 0.91, 0.76): less sepia.
  - Tried and dropped: `MIN_KEEP` 0.01 with `MAX_WIDEN` 16 (fewer far blades): far blades
    got several pixels wide and the frame slower (38.9 ms), ground worse.
  - Wide golden (the scene's own light, the closest match to the reference's): hue
    **49.6 ± 4.4**, sat **0.59 ± 0.10**, val 0.49 ± 0.17, patch24 0.137 (reference 49.2 ±
    4.3 / 0.59 ± 0.08 / 0.46 ± 0.12 / 0.087). Per distance band (rows 53–100 %) near
    hue 50 / val 0.37 (reference 52 / 0.36), middle 51–55 / 0.45–0.54 (47–50 /
    0.43–0.54). Wide midday 57.6 ± 5.5 / 0.62 / 0.42 / 0.038.
  - Ground (look07k): 3.6 % wide / 2.3 % close (matches bare), 44 % / 37 % (tint swap).
  - Timing in this run was not usable: another agent's GPU test shared the GPU ("no grass"
    13.5 ms instead of 5.2). Before the leaf variation (look07e, quiet GPU): High 32.1 ms,
    Medium 24.3, Low 15.6.
- look08 (user: "looks like jungle grass; field grasses are thin and a little curvy"; then
  "the ground has no details or normal maps, almost flat green, no dirt"):
  - **Thin field-grass atlas**: the four leaf cells (broad photographed leaves) are
    regenerated by `tools/grass_atlas.py` as strips of 13–18 thin blades each: 6–10 px of
    128 at the base (2–3 mm on a ~4 cm card), tapering to a fine point, each with its own
    length (55–100 %), lean and a droop growing towards the tip (`x ∝ t^2.2`), roots spread
    across the card (no fan from one point); a few with straw tips. The seed-head cells stay
    photographed. `CELL_AVG` / `CELL_ASPECT` updated; a per-blade twist along the card
    (±0.45 rad at the tip) in `grass_blade`; procedural veins finer. MeadowDemo: cards 4 cm
    wide at 700 / m² (~11 000 visible blades / m², was 1 100 straps), undergrowth 62 % of
    the cards (1.4× wide, lower), `MAX_WIDEN` 8 → 3 and look-down widening 1.75 → 1.3×
    (wide cards made the frame fill-bound: 50 ms at 800 / m² × 5 cm).
  - **Real ground**: close up the ground under the grass keeps its own albedo and normal
    map (only 25 % towards the turf colour, darker and damper under it) and takes the
    canopy colour only far away where it stands in for the turf; the soil relief is no
    longer flattened close up. Worn spots (`grass_worn` / `details::worn`, 1–2 m bare-dirt
    patches) keep a third of the blades and show bare soil. Thatch texture regenerated:
    dark crumbly soil with damp hollows and moss, 1 400 pebbles (4–12 mm, domed), twigs,
    10 000 litter strands (was 36 000, which buried the soil), normal strength 6 → 9.
    Terrain layers blend a second copy of each tile, rotated 0.9 rad and 0.87× the scale,
    by a noise of a few tiles (no visible repeat grid). Before / after crop:
    `grasslook_ground_before_after.png`.
  - Canopy floor a little brighter again (lit 0.8, occlusion 0.6).
  - The canopy relief now fades with `height_variation` (below 0.6), so "uniform" settings
    give one even height again (`grass_varies_across_the_field`).

- look09–10 (lead: pale beige far field, pale blotches, dark olive near, sparse; user:
  "the reference looks softer, everything ... ours looks so sharp with too many details"):
  - Far field: the ground stand-in and far blades less dry (stand-in dry mix 0.55 → 0.35,
    far blades 0.3 → 0.2 + 0.4 z), a warmer far tint (1.16, 0.95, 0.72 over 6–50 m), and the
    scene fog's anisotropy 0.5 → 0.3 (the pale beige was mostly forward-scattered haze
    against the low sun; the midday shot stayed green).
  - Mottling: the ground takes the canopy colour from 4–25 m (was the 24–160 m "far"
    ramp), worn spots rarer (`smoothstep(0.73, 0.85)`) and only near; close up the ground
    is 45 % towards the turf colour.
  - Near colour: root-to-tip gradient lifted (0.9 → 1.1), tip dryness 0.15 → 0.28 (golden
    tips), healthy (0.53, 0.67, 0.21); undergrowth 68 % of the cards at 1.5× width.
  - **Softness**: blade normals lean 70–80 % to the canopy normal (was 45–60 %), the
    thickness-profile normal detail and the midrib / vein / speckle contrast cut by ~60 %,
    photo detail 0.3 → 0.6 towards flat, specular smoothness 0.18 → 0.08, colour variation
    0.5 → 0.2 (variation from the broad patches), seed heads 5 → 3 %; grass shadows soft
    and never fully dark in the canopy (near map: six taps on 1.5 / 3 texel rings, shade
    floor 0.3), contact shadows on grass at 30 % and the scene's contact shadow opacity
    0.8 → 0.3, SSAO on grass at half strength, canopy floor lit 0.85 / occlusion 0.75,
    more light through the blades; alpha-to-coverage softer from mid distance.

## Final (look10 = `grasslook_final_*`)

Renders: `grasslook_final_close.png`, `grasslook_final_wide.png` (golden: the scene's own
light), `grasslook_final_vs_reference.png` (ours | reference, same size), midday variants
and ground masks alongside; ground before / after: `grasslook_ground_before_after.png`.

| | look00 (main) | final |
|---|---|---|
| High / Medium / Low (ms, RTX 4060, 1920×1080) | 34.5 / 30.0 / 20.7 | 31.9 / 24.1 / 15.5 |
| triangles High | — | 37.0 M |
| ground visible wide / close (matches bare) | 2.0 % / 0.9 % | 2.6 % / 1.3 % |
| ground visible wide / close (tint swap) | — | 40 % / 20 % |
| cards / visible blades per m² (High) | 600 cards | 700 cards, ~11 000 blades |

Quality ▸ Grass Density: Low 0.5 (350 cards, ~5 500 blades / m²), Medium 0.75 (525,
~8 000), High 1.0. The "no grass" frame measured 6.1 ms in the final run (5.2 on a quiet
GPU), so the grass itself is ~26 ms on High.

Colour of the grass band, wide golden vs the reference: hue 53.6 ± 5.5 vs 49.2 ± 4.3, sat
0.59 ± 0.11 vs 0.59 ± 0.08, val 0.52 ± 0.15 vs 0.46 ± 0.12, patchiness (24 px blocks)
0.120 vs 0.087. Bands near → far: hue 51 / 51 / 53 / 57 / 55 (reference 52 / 51 / 50 / 47
/ 43), value 0.42–0.48 near (reference 0.36–0.44), 0.50–0.57 middle (0.43–0.54).

The ground-visibility target (< 0.3 %) is not met: the user asked for a real, detailed
ground between the blades instead, so the gaps show soil, thatch, pebbles and occasional
worn spots on purpose and "matches bare" counts them.

Left: the middle distance (20–40 m) is still a little greener than the reference (hue
57 vs 47); the field beyond the 160 m grass draw distance is a flat, hazy stand-in in the
sun's direction (the reference keeps texture to the horizon); midday is greener (63) than
the reference's light; no trees. `foliage_lighting_demo_matches_its_generator` (editor)
fails on main's FoliageLightingDemo data already (density 600 on disk vs 1500 in the
generator); not touched here.

## Round 2 (after the merge into main 18cef2b)

User: "a lot of gaps between each grass, and all of the grass looks alike. Some should be
bigger, some slightly yellowish, some greenish, and at the top they're yellowish. And
there's no ambient occlusion, and the ground shouldn't even be that visible."

- look11: cards 700 → 900 / m², clumping 0.45 → 0.6, tufts' height ±25 % with 12 % big
  tufts (1.3×), per-tuft hue and value (the per-blade jitter cut to a fifth: hue 0.3 /
  value 0.2 per blade, 1.0 / 0.5 per tuft, a wider green ↔ yellow-green range), every tip
  yellowing towards straw (tip dryness 0.45, from a greener base), canopy AO back and
  visible but broad (occlusion floor 0.4, lit floor 0.55 towards the stems' feet; SSAO on
  grass 0.7), ground mostly the canopy colour close up (70 %), worn spots rarer,
  undergrowth 1.8×; far cards fewer (`MIN_KEEP` 0.02 → 0.012). High **41.2 ms** (45.6 M
  triangles) — too slow.
- look11b/c: 750 cards / m², LOD 7.5 → 6.5 m, undergrowth 1.5×, the near grass shadow map
  back to four taps (on a rotated ring of two texels): High 32.3 ms. Variants: contact
  shadows on grass 2.6 ms, near map 2.0 ms, all grass shadows 3.5 ms, AO prepass 0.8 ms.
- look12: no screen-space contact shadow on grass pixels (it was at 30 % and read as crisp
  noise; the canopy occlusion gives the depth), which pays for 800 cards / m²: High 30.5.
- look13: the tufts pull their blades in less (tall 0.6 → 0.45, undergrowth 0.15 → 0.08)
  so the carpet closes between them; the ground close up 85 % canopy colour, darker.
- final: tufts' size variety scales with height variation (uniform settings stay even:
  `grass_varies_across_the_field`), big tufts 1.2×, lit floor 0.62.
- **Round 2 final** (`grasslook_final_*`, overwriting round 1's; step renders
  `grasslook_11_*`, `_12_*`, `_13_*`): High **30.1 ms**, Medium 21.1, Low 14.0 (37.4 /
  24.6 / 12.1 M triangles; the frame without grass 6.0 ms with the GPU shared); 800 cards
  / m² ≈ 13 000 visible blades / m² on High (Medium 0.75, Low 0.5 of that). Ground visible
  2.9 % wide / 1.5 % close (matches bare), 46 % / 23 % (tint swap; the ground close up is
  85 % canopy colour, so that measure now mostly sees blades' gaps of the same colour).
  Wide golden colour: hue 54.0 ± 5.6 (reference 49.2 ± 4.3), sat 0.63 ± 0.13 (0.59),
  val 0.48 ± 0.16 (0.46 ± 0.12), patch24 0.129 (0.087); near band hue 52.5 / val 0.38
  (reference 52 / 0.36–0.41). Left as before: the field past the 160 m grass is a hazy
  stand-in in the sun's direction, the middle distance is a little greener than the
  reference, midday reads greener (64), no trees.

