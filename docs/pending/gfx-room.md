# Cycles-look room (gfx-room)

Goal: realtime images that read like a Blender Cycles render of a white Cornell-style room —
soft wide shadows, soft bounce light, soft AO, faint glossy floor, low-contrast filmic
tonemapping — with rasterization only at runtime (offline baking is an option).

The reference image is not on disk (`E:\MistyDesign\cycles_reference.png` does not exist), so
the comparisons below are against its written description and, as numeric ground truth for
the bounce light, against our own path-traced lightmap of the same room.

## Scene and harness

- `CyclesRoomDemo` (`crates/misty_editor/src/sample_cycles_room.rs`, generator +
  "matches its generator" test like the other samples): 2.4 m cube room open towards −Z,
  white walls (0.8), slightly glossy floor, 0.6 × 1.4 × 0.6 pillar left of centre, 0.7 m cube
  front right, 1.2 × 1.2 × 0.04 canvas standing 6 cm off the back wall. Ceiling light:
  0.6 m square Rectangle light (radiance 16, soft shadows) + an emissive quad of the same
  radiance (no shadows, not in the bake, so the light is counted once). Camera centred in the
  opening at mid height, 42° vertical FOV.
- `CyclesRoomDemo Baked`: the same room with the light Mixed and its bounce light
  path-traced into one 60 KB lightmap.
- Harness: `MISTY_GFX_STEP=<step> [MISTY_GFX_BAKED=1] cargo test -p misty_editor --release --
  --ignored --exact sample_cycles_room::render_cycles_room --nocapture` renders 1024² images to
  `docs/screenshots/gfx/cycles_room_<step>[_baked].png` and prints the mean brightness of seven
  patches (ceiling, back wall, left wall, floor front, pillar front, cube front, floor in the
  cube's shadow). `render_cycles_room_tonemappers` / `render_template_tonemappers` render the
  tonemapper comparisons. `write_cycles_room_demo` regenerates scenes, materials, probes and
  the lightmap. `cycles_room_renders_from_its_baked_data` (GPU, not ignored) checks the
  committed data: lit walls, no crushed blacks, realtime and baked agree.

## Steps

| Step | Image | What changed | How it moved |
| --- | --- | --- | --- |
| 00 | `cycles_room_00_before.png` | Engine defaults: skybox ambient and sky reflections, default bloom (0.6, threshold 0), tonemapper "None". | Far off: a huge bloom blob instead of a panel, blue sky reflected on the floor and box tops inside a closed room, black ceiling, flat grey boxes. |
| 01 | `cycles_room_01_closed_room_agx.png` | Closed room: sky colour, ambient and reflection intensity 0 (only the panel lights). AgX tonemapping, bloom 0.12 / threshold 1. | Neutral white balance and a crisp panel, but direct light only: black ceiling and box faces, too dark overall. Shows how much of the reference is bounce light. |
| 02 | `cycles_room_02_probe_bounce.png` | Box-projected reflection probe filling the room, baked with 4 bounce passes, the emissive quad left out of the captures (no double-counted direct light); +1 EV exposure. | Ceiling and walls glow from bounce light, boxes mid-grey. Flat: no corner darkening. |
| 03 | `cycles_room_03_ao_reflections.png` | GTAO on (radius 0.8, 10 steps, full res, temporal); reflection intensity back to 1 (probe reflections, sky is black). | Slight corner darkening. |
| 04 | `cycles_room_04_ao_stronger.png` | AO intensity 2, radius 1.2. | Wall seams, the floor along the walls and around the box bases darken gently, like the reference. |
| 05 | `cycles_room_05_baked_indirect[_baked].png` | Baked variant: Mixed light, lightmap 24 texels/m, 512 samples, 4 bounces. | Bounce light now varies across surfaces: faces towards the open front are much darker (the opening lets light escape), the pillar front gets a vertical gradient. Closest to a path tracer so far; used as ground truth for the realtime one. |
| 06 | `cycles_room_06_bloom_gloss[_baked].png` | Bloom 0.25 / scatter 0.75, floor smoothness 0.6, lightmap 32 texels/m, 1024 samples. | Glow too strong: the panel loses its edge. Lightmap banding gone. |
| 07 | `cycles_room_07_bloom_softer[_baked].png` | Bloom 0.15 / scatter 0.7. Patch statistics added. | Panel crisp with a soft halo. Realtime vs baked: walls/ceiling/floor within 10 levels, **cube front 115 vs 48** (realtime far too bright). |
| 08 | `cycles_room_08_front_probe[_baked].png` | Second, more important probe near the opening (blended in): it sees mostly the dark opening. | Cube front 57 vs 46 — realtime now matches the baked bounce light everywhere except deep in the cube's shadow (83 vs 59). |
| 09 | `cycles_room_09_floor_gloss[_baked].png` | Floor smoothness 0.7, boxes 0.1. | At this camera angle the floor reflection stays faint (Fresnel ≈ 0.05), as in the reference. |
| final | `cycles_room_final.png`, `cycles_room_final_baked.png` | Front probe box reaches past floor, walls, ceiling and opening so it only blends out towards the back (its fade along the floor showed as a horizontal seam on the cube front); no box projection for it. | Final realtime and baked renders. |

Final patch brightness (sRGB 0..255), realtime / baked:

| ceiling | back wall | left wall | floor front | pillar front | cube front | floor in cube shadow |
| --- | --- | --- | --- | --- | --- | --- |
| 198 / 197 | 185 / 189 | 184 / 179 | 169 / 165 | 108 / 105 | 52 / 44 | 73 / 56 |

Tonemapper comparison on the room: `cycles_room_tonemap_{none,aces,agx,agx_punchy,pbr_neutral}.png`;
on the PP Ready template: `template_pp_ready_{aces,agx,agx_punchy,pbr_neutral}.png`.

## Decisions (with reasons)

1. **AgX and Khronos PBR Neutral tonemappers, AgX looks Base / Punchy.** AgX is what Blender
   (and so the Cycles reference) uses; ACES on the room crushes the box faces and darkens mid
   grey (step 01 / tonemap images), PBR Neutral keeps albedos exact but clips the panel
   harder. AgX Base gives the reference's low-contrast, no-crushed-blacks look. Packed without
   changing the view uniform layout: mode 3 / 4 in `grade.x`, the look in `color_filter.w`.
   Look and the new white balance fields are left out of files at their defaults, so existing
   scenes keep their exact text.
2. **White balance (Temperature, Tint) in Color Adjustments**: a von Kries adaptation in LMS
   (CAT02 matrix) from a shifted daylight chromaticity to D65, reduced to linear-RGB gains of
   white and folded into the colour filter on the CPU (no shader cost), normalised to keep
   luminance. Post Exposure and Contrast already existed.
3. **PP Ready template → AgX Punchy, not AgX Base.** Base washes out the sunset (it was tuned
   for ACES); Punchy keeps ACES's contrast while the sun and bright clouds roll off to white
   instead of skewing hue. The Standard template has no tonemapping (post off) and is
   unchanged. The CyclesRoom demo uses AgX Base. Thumbnail re-rendered.
4. **Realtime bounce light = baked reflection probes, ambient off.** The closed room sets
   ambient colour/intensity and sky to 0 — the environment ambient can already be disabled
   (Color source, intensity 0) and probes replace it inside their boxes; no engine change was
   needed. A single probe's SH can't vary across the room, so a second probe near the opening
   carries the darker bounce light there. When the screen-space GI work lands it should replace
   the probes' diffuse for the spatial detail they lack.
5. **Probe bake with several bounces, emitter excluded** (sample-side `bake_probe`): the stock
   `bake_scene` does at most 2 passes, which leaves a white room too dark (albedo 0.8 needs ~4
   bounces); the emissive quad would otherwise be counted twice (once as the rect light's
   direct light, once in the SH).
6. **Baked option = Mixed light + lightmap** (realtime direct light, specular and soft shadows;
   path-traced bounce light at 32 texels/m, 1024 samples, 4 bounces → 60 KB). Fully Baked
   would lose the panel's specular highlight and the shadow quality would depend on texel size.
7. **AO**: the existing GTAO already has multi-bounce colour (white rooms don't go grey),
   Lagarde specular occlusion, bilateral denoise and temporal accumulation; it only needed
   tuning (intensity 2, radius 1.2 m, 10 steps, full resolution). No AO code changes.
8. **Area-light shadows** were already sized by the light (`soft_shadows.rs`: rectangle
   shadow radius = half the mean side, PCSS). No hook needed. Filtering quality is the shadow
   agent's area; at this light size the penumbrae show faint dither noise (acceptable as grain).

## Remaining gap to the reference

- Realtime bounce light is still per-probe: the floor deep in the cube's shadow is ~17 levels
  brighter than the path-traced lightmap (73 vs 56), and there's no colour bleeding / per-surface bounce
  gradient beyond what two probes give. Screen-space GI (parallel work) or more probes would close it.
- Contact shadows/AO are screen-space: off-screen occluders don't darken.
- Penumbrae are PCSS approximations: slightly darker cores and faint dither noise compared
  with Cycles' exact area shadows (shadow-filtering work is with the other agent).
- Floor reflections come from a box-projected probe: blurry, no occlusion of the reflected
  boxes (SSR is with the other agent).
- No film grain; Cycles' faint noise isn't reproduced (not needed).
