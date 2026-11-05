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

## Step log

- 00_before: baseline shots.
- 01_first: shade_grass, front-to-back, per-blade fade, arc bend, travelling gusts, colour patches,
  ground tint, grass shadows. High 25.7 → 17.3 ms, Medium 23.5 → 15.0, Low 19.1 → 11.1 (same run
  conditions as the baseline). Ground still showed pale in dry patches (ground tint 0.7 left 30 % of
  the bright soil; now 0.9 and darker).
- Grass shadows looked absent in MeadowDemo: the golden-hour shot sits in a cloud shadow (Cloudy
  preset, shadow opacity 0.6) and the old 12 cm receive filter blurred 2 cm blade shadows away.
  Filter narrowed; the proof shot turns the cloud shadow off over the spot.
