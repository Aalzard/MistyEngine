# Grass realism (branch grass-realism)

User feedback on MeadowDemo: "it still doesn't look realistic because all of the grass looks exactly
the same". Goals: colour and size variation, travelling wind waves, no ground showing between blades,
grass shadows, all without losing performance. Later asks from the coordinator: real blade-shaped sun
shadows first (with a side-by-side proof), grass in the screen-space prepass, TAA smear from wind,
stippled thin stems, a higher density cap.

## Measuring

- `grass_cost` (ignored, `render/grass_gpu_tests.rs`): MeadowDemo main shot at 1920×1080, median GPU
  time of 120 frames after 40, per quality level, with and without grass; then ground visibility.
- **Ground visibility** = share of the pixels that show ground where grass grows (CPU ray march of
  each pixel onto the heightfield: detail density > 50 %, within the grass draw distance) whose colour
  is the same as in the same render with zero blades (grass shadows off, no TAA). The first version of
  the metric counted the mountains and the field past the draw distance as "ground" (30–40 %), so it
  was replaced; the baseline was re-measured with the final metric on a detached worktree at 958ba90.
- GPU times on this machine move by ±2–3 ms between runs (the user's editors share the GPU, a build
  running at the same time adds more). Compare numbers from the same run only.
- `grass_shadow_proof` (ignored) writes `docs/screenshots/gfx/grass_shadows_on_vs_off.png` and times
  each grass-shadow tier; `grass_shots` (ignored) writes the four comparison shots per step
  (`MISTY_GFX_STEP=<name>`).

## Baseline (958ba90)

| quality | grass | no grass | grass triangles |
|---|---|---|---|
| High | 25.7 ms | 5.0 ms | 12.9 M |
| Medium | 23.5 ms | 4.1 ms | 12.8 M |
| Low | 19.1 ms | 2.2 ms | 12.9 M |

Grass cost about 20 ms on every level, Low included. Experiments on the baseline:
- all blades degenerate in the vertex shader (vertex cost only): grass +1.4 ms;
- trivial fragment shader (raster + texture only): grass +4.3 ms;
so **~14 ms was the full `shade()` per grass fragment** (probes, SSGI/SSR reads, lightmap and light
probe lookups, sky reflection, rect lights) multiplied by overdraw.

Ground visible (final metric): 8.7 % wide golden, 4.4 % close golden.

## Decisions

1. **`shade_grass`** (grass.wgsl): hemisphere ambient + SSAO + canopy occlusion, lights with the cheap
   4-tap shadow (`fast_shadow_factor`), cloud shadow, transmission, a GGX sheen, fog and volumetric
   fog. Probes, SSGI/SSR, lightmaps and rect lights are left out. This pays for everything else.
2. **Nearest chunks first** (front-to-back) so blades behind fail the depth test early.
3. **Per-blade LOD fade:** the drop-out key is stored in the blade (`extra.x` = key, + 2 for a seed
   stem). The CPU still draws the prefix of each key-sorted chunk its nearest point needs; the vertex
   shader shrinks every blade whose key passes the share kept at its *own* distance, so thinning is
   continuous (no popping) and excess blades become zero-area. Far blades widen up to 4× (was 3×).
4. **Length-keeping bend:** the card follows a circular arc whose total angle comes from lean + wind,
   so the tip never stretches.
5. **Wind:** `grass_gust` = broad value-noise waves travelling along the wind (stretched across it)
   plus faster smaller puffs; per-blade flutter whose phase runs along the wind; tall blades give more
   than short ones, seed stems least. Gusts also lift albedo and sheen a little so the waves show as
   moving lighter bands. Settings: Wind Direction, Gusts, Gust Size, Flutter (plus Strength/Speed).
   A **Wind Zone** component (Miscellaneous; its +Z axis = direction; Main / Turbulence / Pulse
   Magnitude / Pulse Frequency multiply the grass's own values) overrides direction and scales; the
   first enabled zone wins. `Environment.grass_time` fixes the wind clock for tests.
6. **Colour variation** (`grass_field.wgsl`, same noise ported to `details::patches` on the CPU):
   soft lush / dry patches of Patch Size metres, Lush Colour + Lush / Dry amounts, per tuft and per
   blade hue/value jitter (Colour Jitter), root-to-tip gradient (darker greener roots, paler tips).
7. **Height / density variation** in the scatter: heights follow the lush patches (taller, fuller) and
   a second area noise, dry patches shorter (Height Variation); blade counts follow the same fields
   (Density Variation). The canopy top rolls instead of standing at one height.
8. **Hiding the ground:** (a) the terrain gets the detail map (binding 7) and the grass colours, and
   under grass its albedo becomes the grass's base colour in deep shade (Ground Tint; lighter far away
   where blades are thin, normal map flattened); (b) undergrowth blades are 1.6× wider and arch over
   more; (c) blades the camera looks down on grow up to 1.9× wider (view-angle compensation); (d) the
   canopy darkening by height stays, so gaps read as deep shadowed grass.
9. **Grass shadows:** a depth-only, alpha-tested grass caster pipeline (`grass_shadow.wgsl`, coarse
   three-segment blades, a share of the drawn ones, slope bias 2.5) drawn into the cascades that start
   within the caster range; casters shrink away over the last 40 % of the range so there is no seam.
   Quality ▸ **Grass Shadows**: Off (Low level), Low = 12 m / 35 %, Medium = 20 m / 50 % (High level),
   High = 32 m / 60 % (Ultra). The grass layer has its own **Cast Shadows** switch. Receiving: the
   4-tap filter radius dropped from 12 cm to 2.5 cm (+8 %/m) so single blades cast blade-shaped
   shadows; the ground (terrain) keeps its normal soft-shadow path.
10. **Grass in the screen-space prepass** (`grass_prepass.wgsl`): blades within 12 / 20 / 30 m (AO
    tier) write depth and the lit pass's normal, alpha tested, so GTAO, contact shadows, the shadow
    mask and SSGI/SSR see blades instead of the ground behind them.
11. **Density cap** raised from 600 to 1500 blades/m².
12. **Near-camera grass shadow map** (`grass_near_shadow.rs`): its own sun depth map of the grass
    alone over a 24 m square around the camera (Quality ▸ Grass Shadow Map: Off on Low, 1024 on
    Medium, 2048 on High, 4096 on Ultra), every blade (full density, coarse three-segment cards,
    alpha tested), the centre snapped to whole texels of a world-fixed grid (no shimmer). Lit
    shaders multiply a 3×3 PCF of hardware-filtered taps into the sun's shadow (`grass_near_shadow`
    in mesh.wgsl, for meshes, terrain and grass) and fade it out over the last 30 % of its radius,
    where the cascade casters fade in (they leave the near square to it). Group-1 bindings 8/9.
13. **Grass density moved into the terrain normals' w** instead of its own texture: the terrain
    pipeline hit the 16 sampled-textures-per-stage limit once the near map was added.
14. **Blade texture:** coverage-preserving atlas mips (per cell, binary-searched alpha scale so the
    share of texels passing the 0.5 test stays the mip-0 share; thin blades no longer fade or
    bloat far away), 16× anisotropy, stronger photo detail (75 % instead of 40 %, fading only 60 %
    far away), procedural veins, a midrib highlight and speckles close up, and the atlas alpha's
    slope across the blade used as a normal map (rounded edges, midrib crease) and as a thickness
    map (thin edges transmit more light).
15. **Gaps:** undergrowth (55 % of blades) is no longer pulled into the tufts (15 % instead of 60 %
    of the clumping pull), so it fills between them; tall blades stay clumped. MeadowDemo density
    450 → 600 /m², clumping 0.6 → 0.4.
16. **Ground under the grass:** MeadowDemo's ground is a generated tileable PBR set
    (`MeadowThatch*.png`, 1024², 2.2 m tile): dark soil with moss under 16 000 strands of straw,
    olive and green litter, with height and a normal map from it. Every terrain layer gets a
    macro variation (the same texture 7× larger relative to its mean colour) against tiling. The
    ground tint keeps the litter's light and dark spots and flattens the normal map under dense
    grass. The old `MeadowGround*.png` are no longer referenced (left in place).
17. **Thin stems:** blades thinner than 0.8 px are drawn 0.8 px wide and proportionally more
    transparent, and with TAA the alpha-to-coverage value gets a per-frame noise so the history
    averages it instead of showing a fixed dot pattern.

## Step log

- 00_before: baseline shots.
- 01_first: shade_grass, front-to-back, per-blade fade, arc bend, travelling gusts, colour patches,
  ground tint, grass shadows. High 25.7 → 17.3 ms, Medium 23.5 → 15.0, Low 19.1 → 11.1 (same run
  conditions as the baseline). Ground still showed pale in dry patches (ground tint 0.7 left 30 % of
  the bright soil; now 0.9 and darker).
- Grass shadows looked absent in MeadowDemo: the golden-hour shot sits in a cloud shadow (Cloudy
  preset, shadow opacity 0.6) and the old 12 cm receive filter blurred 2 cm blade shadows away.
  Filter narrowed; the proof shot turns the cloud shadow off over the spot.
- 02_meadow: MeadowDemo regenerated with the new settings and a Wind Zone; colour patches and wind
  waves read at midday; the golden-hour wide shot still showed pale sparse dry patches.
- 03_dense: density 450 → 600, clumping 0.6 → 0.4, dry 0.5 → 0.4, ground under grass darker and
  keeping the soil's detail.
- User feedback on 02: blades lack texture, gaps between clumps, ugly ground, low-quality shadows.
  Steps 12–17 above answer it, in the order asked: near-camera shadow map
  (`grass_shadows_near_map.png`: cascade casters only | near map), blade texture
  (`grass_04_texture_vs_03_close_*.png`), gaps and ground (`grass_05_ground_*`).
- 05_ground: final shots; `grass_05_vs_00_<shot>.png` are before (left) / after (right).

## Results (MeadowDemo, 1920×1080, RTX 4060; before = 958ba90, same session, back to back)

| quality | before | after | grass part before → after |
|---|---|---|---|
| High | 25.7 ms | 32.0 ms | 20.7 → 26.9 ms |
| Medium | 23.6 ms | 27.8 ms | 19.5 → 23.6 ms |
| Low | 18.7 ms | 19.1 ms | 16.6 → 16.8 ms |

The after column has 33 % more blades (600/m²) and everything new on: grass in the AO prepass,
cascade grass casters and the 2048² near grass shadow map on High (Medium: 1024² map, 12 m casters).
Low (no grass shadows, no prepass grass) costs the same as before with the denser field, because the
lean `shade_grass` pays for it. Earlier in the session, at the old density and without the near map
and prepass, High was 17.3 ms (−8.4 ms).

Grass shadow costs on High (proof run; GPU timings drift ±2 ms between runs): cascade casters
+0.5–1.7 ms, near map +1–3 ms (1024² ≈ 2048²; 4096² ≈ +1 ms more — the cost is the casters'
geometry, not the resolution).

Ground visible between blades (final metric): wide golden 9.1 % → 1.9 %, close golden 4.5 % →
0.8 %. Synthetic test field: 17.3 % → 8.7 % (`grass_hides_the_ground` asserts < 20 %).

## Tests (`render/grass_tests.rs`)

- `grass_casts_and_receives_shadows`: grass shadows darken the field; a cube's shadow darkens the
  grass beside it by > 15 %; casters add triangles only when on.
- `grass_varies_across_the_field`: block colour spread (per row) > 1.3× a uniform field and > 10;
  1 m cell mean heights spread > 2× a field without height variation.
- `wind_waves_travel_across_the_field`: frames differ; the change between frames 1→2 reappears
  shifted downwind between frames 2→3 (cross-correlation peak > 3 px).
- `grass_hides_the_ground`: < 20 % ground visible in the standing shot.
- `grass_shadows_off_cost_nothing`: Low has grass shadows off; with them off (quality or layer
  switch) the frame draws exactly the triangles of a field that casts none.
- `grass_near_shadow::tests`: the near map stays on its texel grid and covers the camera.
- `wind_zone::tests`: the first enabled zone sets the wind; old files load.

## Left / not done

- **TAA motion vectors for wind:** there is no velocity buffer; moving blades still smear somewhat
  under TAA (the coordinator measured ~20 % of pixels changing > 8 levels per frame at wind 0.12).
  A per-vertex previous-frame displacement needs a motion target the TAA pass reads; not started.
- **Faces away from a low sun going dark/blue:** not addressed beyond SSGI now seeing near blades.
- **Several species / wildflowers per terrain:** not started (one detail layer per terrain).
- High costs +6 ms over before at 33 % more blades; a cheaper near-map caster LOD (half density past
  6 m) and a lower MeadowDemo density would bring it back if needed.
- The near map is grass-only: other casters keep the cascades' resolution.
