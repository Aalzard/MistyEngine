# Foliage lighting look-dev (gfx-foliage)

Goal: a small patch of grass that reads like a nature macro photo in the low golden-hour sun —
glowing translucent blade edges, deep soft shade between blades, warm rim light, no flicker or
aliasing — built only from scene content and lighting/post settings. The blade shading itself
(mesh.wgsl blade code, grass.wgsl, terrain grass) belongs to the grass session and is not
touched here; rendering problems found on the way are listed at the end for it.

## Scene and harness

- `FoliageLightingDemo` (`crates/misty_editor/src/sample_foliage_lighting.rs`, generator +
  `foliage_lighting_demo_matches_its_generator`): a 40 × 40 m terrain of generated soil
  (`Assets/Textures/Foliage/Soil*.png`: humus lumps, damp hollows, pebbles, straw; albedo,
  normal, height; 1.2 m tile) with a hand-made detail map: a ~2 × 2 m dense patch with a wobbly
  outline, sparse tufts around it on bare soil, and a thinner meadow beyond ~5 m. The grass is
  the MeadowDemo grass system (atlas blades, tufts, seed heads, translucency). Under the patch
  the ground is the meadow litter photo darkened (`ShadedLitter.png`, see step 02). A second
  terrain object (`Tall Grass`, same heights and splat map, ground hidden under the main one's
  identical surface, no shadows) carries a few clumps of taller stems with more seed heads.
  Five sunk, flattened pebbles (`Assets/Materials/Foliage/Pebble.mat`).
- Cameras: `Main Camera` 0.55 m above the soil just outside the patch, 10° down, 40° vertical
  FOV, looking towards the sun; `Wide Camera` (off) 1.6 m up, framing the whole patch.
- Harness: `MISTY_GFX_STEP=<step> cargo test -p misty_editor --release -- --ignored --exact
  sample_foliage_lighting::render_foliage_lighting --nocapture` renders 1600 × 900 images
  `docs/screenshots/gfx/foliage_<step>_{golden,midday,off,wide}.png` (32 frames, so TAA and
  the temporal shadow mask settle) on the High quality level and prints the mean brightness of
  the top and bottom halves. `MISTY_GFX_LOOKS=golden,wide` renders only some;
  `MISTY_GFX_OFF=fog,ssgi,ao,contact,bloom` switches effects off for diagnosis;
  `MISTY_GFX_QUALITY=medium|ultra` changes the level. `golden` = backlit golden hour (the
  committed scene), `midday` = high sun from the side, `off` = the golden-hour sun with AO,
  contact shadows, SSGI, fog, bloom and vignette off, ACES, hard shadows, no soft-shadow
  temporal denoise and no TAA, `wide` = the golden hour from the wide camera.
  `foliage_lighting_frame_time` measures GPU time at 1920 × 1080.

## Steps

Kept images (the others were looked at and deleted to keep the repo small): `foliage_00_first_{golden,wide}`, `foliage_02_shaded_litter_punchy_golden`, `foliage_05_clods_wide`, `foliage_06_ambient_wide`, `foliage_07_low_camera_backlight_golden`, `foliage_diag_calm_golden` (step 08 set-up with `MISTY_GFX_CALM=1`, no wind), `foliage_final_{golden,midday,off,wide}`. In the harness output each `flicker:` line belongs to the shot named on the line after it.

| Step | What changed | How it moved |
| --- | --- | --- |
| 00 | First pass: sun 14° up, from ahead-right of the camera, warm (1, 0.74, 0.48) × 3.2, soft shadows; procedural sky; AgX Base, bloom 0.18 / threshold 1, vignette 0.18; GTAO 1.6 / 0.3 m, contact shadows 0.1 m, SSGI on; volumetric fog (mean free path 260 m, g 0.7). Patch on bare soil to the horizon. | Blades glow at their edges, but the whole frame is washed out: a pink haze over everything, the ground a pale beige sheet; the effects-off ACES baseline looks more dramatic than the "on" image. The patch is thin (the grass system caps density at 600 blades/m²), the soil reads as polka dots, pebbles render near-black, the barren horizon looks like a desert. |
| 01 | Tall Grass overlay terrain (taller stems, 25 % seed heads, own seed: also doubles density in the core); thinner meadow past a ~5 m clearing as background; greener blades; fog mean free path 1200 m, g 0.55, top 6 m; fewer, dustier pebbles and less straw in the soil. | Tall panicles against the sky, a believable background. Diagnosis (`MISTY_GFX_OFF`): switching SSGI off changes the image by 0.05 levels, fog 2.3, AO + contact 0.6 — the ground under and between the blades is fully sunlit because blades cast no shadow-map shadows and are not in the AO / SSGI prepass (see bugs 1–2). |
| 02 | Ground under the canopy: the meadow litter photo at 42 % brightness (the canopy's shade painted into the layer, since the blades can't shadow it); AgX Punchy; pebbles without the cliff normal map. | Much deeper, more photographic: dark base between the stems, bright translucent blades and seed heads on top, warm low-contrast sky. Pebbles still dark navy on their shaded side (lit only by the blue sky ambient). Soil normal map too strong (crinkled-foil sheen in the backlight). |
| 03 | Soil normal map half as steep; irregular pebble outlines, fewer pebbles, thinner darker straw; procedural sky ground colour = warm soil bounce (0.42, 0.31, 0.21); atmosphere 0.8. | Midday shot already reads well (green blades, pale seed heads, dark litter under the canopy). Golden: the soil beside the patch still shines like crinkled foil and looks like rippled sand. |
| 04 | Smoothness 0 on both ground layers; litter normal 0.5; bluer sky tint. | The foil sheen is diffuse grazing light on the relief, not specular: no change. The "bluer" tint made the sky more violet (reverted). |
| 05 | Soil relief rebuilt from cellular clods (~1.5 cm cells, dark cracks, rounded tops) instead of smooth fBm; pebbles half soil-coloured; pebbles not static. | Wide shot: cracked, crumbly earth instead of sand ripples. Pebbles still render near black in the backlight (not a static-batching issue). |
| 06 | Environment intensity 1.8 (the procedural sky dims its ambient strongly near sunset). | Shaded faces lift a little (6.9 levels mean). The pebbles' shaded faces turn saturated blue: they only get sky ambient. Diagnosis: SSGI ×4 changes the wide image by 0.26 levels, so the warm soil bounce never reaches them either (see bugs). |
| 07 | Camera 0.45 m up, 1.5 m from the patch centre, 7° down; sun 12° up and 15° right of the view axis (now in frame, top right); translucency 1.0; white balance +10 (warmer); pebble tint warmer. | The composition becomes a golden-hour macro: the sun disc and its glow in the sky, panicles and spikes silhouetted against it, blades lit through in yellow-green. Too bright overall; the shade between stems is not deep. Thin stems show a faint stipple (alpha-to-coverage pattern through TAA). |
| 08 | Exposure −0.4 EV, bloom 0.25. Frame-to-frame stability metric added to the harness. | Richer, the sun glows. Stability: ~20 % of pixels change by > 8 levels per frame in the close shots, 0.2 % in the wide one; with `MISTY_GFX_CALM=1` (no wind) 0.04 % with TAA and 0.00 % without. So no shading flicker: it is the wind moving the blades, and TAA (no motion vectors for the wind) softens the near blades in motion (bug 3). |
| 09 | Screen-space Sun Shafts (0.6) between the stems; environment intensity 1.3 (deeper shade at the stem bases). | Shade better. The shafts only add a salmon-pink disc around the sun, no visible streaks. |
| 10 | Shafts 0.35, yellower tint. | Still a pink halo, not rays. |
| final | Sun Shafts off (dropped). | Final images. |

## Final set-up (why)

- **Sun** 12° up, 15° right of the camera's view axis, colour (1, 0.74, 0.48) × 3.2, Soft
  shadows (light-shape PCSS, temporal denoise from High up), contact shadows on: in frame at the
  top right, so the patch is backlit and the blades transmit it (translucency 1.0).
- **Sky** procedural, sun disc High Quality; its ground colour set to a warm soil bounce, and
  ambient × 1.3 (the procedural sky darkens its ambient a lot at sunset; 1.8 lifted the shade
  between stems too much).
- **Post**: AgX Punchy (Base looked washed out against this low-contrast haze; Punchy keeps the
  dark stem bases and saturates the glow), exposure −0.4 EV, white balance +10, bloom 0.25 /
  threshold 1 / scatter 0.65 (the sun's glow, glints on seed heads), vignette 0.18.
  GTAO 1.6 / 0.3 m / 25 % on direct light (only affects ground and pebbles, see bug 1),
  contact shadows 0.1 m / 16 steps, SSGI on (radius 0.8 m, thickness 0.08 m; no visible effect
  here, bug 1), thin volumetric haze (mean free path 1200 m, g 0.55, 6 m deep): it lightens the
  horizon band only.
- **Ground**: painted canopy shade (darkened litter under the grass) is what makes the "deep
  soft shade between blades": the engine can't produce it (bug 1).
- **TAA** on (High), **quality** High.

## GPU frame time (main shot, 1920 × 1080, RTX 4060)

`cargo test -p misty_editor --release -- --ignored --exact
sample_foliage_lighting::foliage_lighting_frame_time --nocapture` (60 frames after 15 warm-up):

| Level | GPU mean | median |
| --- | --- | --- |
| High | 14.67 ms | 14.67 ms |
| Medium | 13.46 ms | 13.50 ms |

## Rendering bugs / limitations found (for the grass session; not fixed here)

1. **Grass blades are invisible to every screen-space and shadow pass.** They are drawn only in
   the main colour pass (`render/mod.rs` "Draw Grass"): not in the shadow maps, not in the AO /
   contact-shadow depth-normal prepass (`screen_space.rs::prepass` draws meshes and terrains
   only), so not in the SSGI / SSR trace inputs or the temporal shadow mask either. Effects:
   blades cast no shadows on the ground or on each other (the ground under a dense patch is
   fully sunlit — this demo paints the shade into the ground texture instead); GTAO and contact
   shadows never darken between blades; SSGI can't bounce soil light onto blades (switching it
   off changes the golden shot by 0.05 levels, ×4 intensity by 0.26). Worse, blade pixels
   sample the AO / SSGI / shadow-mask textures at their screen position, which hold the values
   of the *ground behind them*.
2. **Shaded faces get only sky ambient at low sun.** With SSGI unable to help (above) and the
   procedural sky's ambient scaled down by `daylight()` near sunset, faces turned away from the
   sun (pebbles, the backs of blades) go dark and blue (the pebbles in `foliage_06_ambient_wide`
   are saturated blue). Not grass-specific, but it shows up most on foliage scenes.
3. **No motion vectors for wind.** TAA reprojects grass by depth only, so wind-animated blades
   near the camera smear: the step 08 golden shot was visibly softer than the same frame with
   `MISTY_GFX_CALM=1` (`foliage_diag_calm_golden`; crops of the mid-left stems), and ~20 % of pixels change by > 8 levels per
   frame at wind 0.12 (0.04 % calm).
4. **Thin stems stipple.** Seed-head stalks and far blades show a faint regular dot pattern
   (alpha-to-coverage) that TAA doesn't fully resolve (sky behind the panicles in the golden
   shot).
5. **Density cap per terrain.** `details::MAX_DENSITY` (600 blades/m²) is too low for a dense
   macro patch; this demo layers a second terrain with its own seed to double it (and to add a
   taller clump layer). A per-terrain multiplier or several detail layers would be cleaner.
6. **One grass kind per terrain.** No wildflowers or second species (so none here); the Tall
   Grass overlay is a workaround.
7. Screen-space Sun Shafts with the sun in frame give a salmon-pink disc rather than streaks
   between occluders (step 09/10) — maybe because the sky source is the bright, near-white disc
   tinted by the AgX path; not investigated.

## What's left

- Re-tune once grass shadows / prepass land (bug 1): then remove the painted canopy shade, turn
  the ambient back towards 1.0 and let SSGI do the soil bounce.
- An HDRI sunset sky would give a bluer zenith than the procedural sky's violet at 12°.
