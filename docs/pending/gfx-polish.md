# gfx-polish — SSGI stripes fix and CyclesRoomDemo retune

Branch `gfx-polish` (worktree `E:\MistyWork\gfx-polish`), on top of main with AgX +
CyclesRoomDemo, SSGI/SSR and temporal soft shadows merged, plus main's uncommitted change
turning SSGI and SSR on in the demo's volume.

## The bug

With SSGI + SSR + High soft shadows on (`cycles_room_all_on.png`), walls and ceiling showed
regular light/dark diagonal bands (~20–40 px apart, ~30°), and the room came out too bright and
flat (patches ceiling 202, back wall 219, left wall 203, floor front 181, pillar front 140, cube
front 63, floor right of cube 126; before SSGI 198/185/184/169/108/52/73).

### Isolating it

`render_cycles_room` gained `MISTY_GFX_OFF=ssgi,ssr,ao,mask,soft,panel` (debug: turn one effect
off, or drop the emissive light panel). Patches (ceiling / back / left / floor front / pillar /
cube front / floor right of cube):

| off            | patches                         | bands |
|----------------|---------------------------------|-------|
| nothing        | 202 219 203 181 140 63 126      | yes   |
| ssgi           | 198 185 184 168 108 52 85       | no    |
| ssr            | 202 219 203 181 141 63 119      | yes   |
| ao             | 200 217 199 178 128 62 124      | yes   |
| mask (temporal shadows + denoise) | 202 219 203 181 140 63 125 | yes |
| soft           | 202 219 203 181 140 63 122      | yes   |
| ssr, ao, mask  | 200 217 199 179 129 63 132      | yes   |
| panel          | 134 185 184 170 124 61 119      | -     |
| panel, ssgi    | 128 183 184 168 108 52 85       | -     |

Only SSGI matters, for both the bands and the brightness. The frame index does advance per
`render()` on the same target in every effect (SSGI `state.frame`, shadow mask `state.frame`,
AO `state.frame`), in the harness and in the editor (Game and Scene views each keep one
`RenderTarget` per view, recreated only on resize, so the per-target frame counters and histories
keep running while the views render continuously).

### Root causes (three, all in SSGI)

1. **Bands: noise aliasing at half resolution.** `cs_gi_trace` rotated its R2 ray sequence and
   step jitter by interleaved gradient noise of the *full* pixel centre (`pc = 2t + 0.5` for GI
   texel `t`). IGN sampled on every other pixel is no longer blue: its spectrum has a strong
   peak at a 9.7-texel (19 px) period at 29.5° (numerically checked), so neighbouring texels get
   nearly the same directions and jitter, coherently over tens of pixels. The per-frame offset
   shifts that pattern uniformly, so 24 accumulated frames keep a band-shaped bias and the
   à-trous (±8 texels) can't remove it. In the room the light panel's huge radiance made it
   plain.
   **Fix:** IGN of the GI texel coordinate (`t`, the image's own grid), per-frame offset
   `5.588238 · (frame % 64)` (Jimenez 2014), second component decorrelated by a constant offset.
2. **Too bright / flat: double-counted ambient.** `screen_gi_apply` skipped the traced
   visibility whenever SSAO was on (to avoid double darkening), so the full probe diffuse — which
   already holds the room's bounce — was kept *and* the traced bounce added on top for the same
   directions. **Fix:** the ambient keeps only the share of rays that hit nothing; with SSAO on,
   the darker of the two visibilities wins (`ambient · min(traced / ao, 1)`, ambient is already
   × ao), never their product.
3. **Too bright, washed-out shadows: the light counted twice.** The ceiling light is a
   Rectangle light plus an emissive Quad (Contribute GI off) showing the emitter. SSGI rays that
   reached the panel brought back its emission (radiance 16) — the rectangle light's direct
   lighting a second time — and the colour-history mips spread the panel's glow into every wide
   cone near it. Hiding the panel dropped the back wall from 216 to 185.
   **Fix:** Mesh Renderer ▸ Contribute GI now also means "sends light through SSGI":
   `MaterialParams::contribute_gi` (set by the draw list from the renderer), packed in
   `Draw.lightmap.y`; the depth + normal prepass writes a negative `w` for such surfaces;
   `gi_march` returns a black hit for them (they block, send nothing), and the first colour
   mip step leaves their pixels out of its 2×2 average (`cs_color_down` with the prepass normals
   bound for level 0 → 1). SSR still reflects them (`surface_at` reads `|w|`).

### Tests

- `misty_editor` `sample_cycles_room::cycles_room_ssgi_leaves_no_bands_on_the_walls` (GPU):
  the committed realtime room at 1024², half-res SSGI on vs off, 24 frames, quality level set
  explicitly (High; no TAA once that lands), the light panel forced to Contribute GI on (the
  hardest case). Banding = RMS of a 5×5 minus 31×31 box difference of the luminance over the back
  wall and the left wall. Before the fix: back wall 1.50 (SSGI off 0.23) → fails; after: 0.41 /
  0.44 (off 0.23 / 0.58) → passes (threshold off + 0.6). Smaller engine-side scenes at 320×240 (open walls,
  a glowing strip, a checkerboard of glowing tiles) did not show the bands clearly enough to
  discriminate, so the regression lives on the room.
- `misty_engine` `render::screen_trace::gpu_tests::emitters_without_contribute_gi_send_no_bounce`:
  a glowing panel on a dim floor; SSGI with Contribute GI off leaves the floor before it as
  without SSGI (54.7 → 51.2), with it on the glow bounces (→ 93.9).

### Editor views

The fix is in the engine passes, so the Game and Scene views get it too. Their frame index path
was already right: `viewport.rs` keeps one `RenderTarget` per view (recreated only when its size
changes), and SSGI, SSR, the shadow mask and AO each keep their history and frame counter in
that target's slot, advancing once per `render()`. A resize or camera cut restarts the
accumulation (as intended).

### Not changed, noted

- The reflection-probe bake of the demo already leaves the emissive panel out of its captures
  for the same reason (`bake_probe`); the generic probe bake does not look at Contribute GI.
- Rays that leave the screen take the probes' diffuse irradiance in the surface normal's
  direction for their whole share, although they mostly head for the room's dark opening. That
  still over-lights surfaces facing the camera a little (pillar and cube fronts). A per-ray probe
  radiance lookup in the trace would fix it; out of scope here, the demo lowers SSGI intensity.
- `Draw.lightmap.y` is the new "no GI" flag; `.z`/`.w` stay free.

## Tuning CyclesRoomDemo (realtime)

Patches: ceiling / back wall / left wall / floor front / pillar front / cube front / floor right
of cube (in the cube's shadow). Path-traced baked variant reference: 197 / 189 / 179 / 165 / 105 /
44 / 56. Pre-SSGI realtime: 198 / 185 / 184 / 169 / 108 / 52 / 73.

| step | settings | patches |
|------|----------|---------|
| stripes (main) | SSGI 1, exp 1, AO 2 / 1.2 m | 202 219 203 181 140 63 126 |
| noise fix | | 202 217 200 180 140 63 126 |
| + visibility fix | | 200 216 196 178 129 63 125 |
| + emitter skipped at hits | | 199 195 188 172 126 61 121 |
| + emitter left out of the colour mips | | 199 187 184 170 124 61 119 |
| SSGI 0.7 | | 199 187 184 170 121 58 110 |
| SSGI 1.3 | | 200 189 186 173 133 64 129 |
| thickness 0.25 | | 199 186 184 170 120 59 117 |
| radius 1.5 | | 198 186 184 170 128 64 115 |
| AO 3 | | 199 187 184 171 125 60 111 |
| exposure 0.85 | | 195 183 179 166 119 57 114 |
| AO direct 0.5 | | 199 186 182 167 123 60 110 |
| SSGI 0.7, exp 0.85, AO 3, direct 0.5 | | 195 181 178 162 115 52 84 |
| SSGI 0.7, exp 0.9, AO 3, direct 0.5 | | 196 182 180 163 117 54 86 |
| SSGI 0.7, exp 0.92, AO 3 / 1.8 m, direct 0.5 | | 196 179 180 161 117 51 78 |
| **SSGI 0.6, exp 0.92, AO 3.5 / 1.6 m, direct 0.6** (chosen) | | **196 179 179 159 115 50 69** |

Decisions:
- SSGI intensity 0.6: the knobs barely move the walls once the double counting is gone; lower
  intensity mainly pulls down the camera-facing surfaces the off-screen fallback over-lights.
- Post exposure 1.0 → 0.92 (0.08 stop): ceiling and walls back to the reference.
- AO 3.5, radius 1.6 m, Direct Lighting Strength 0.6: darker seams and corners and a wider
  contact shade under the boxes, as in the path tracer (floor in the cube's shadow 119 → 69).
- The baked variant is the reference: it keeps AO 2 / 1.2 m, no direct AO, exposure 1.0 (its
  lightmap already holds the occlusion; the stronger AO crushed its shadow to 36). It now renders
  197 / 189 / 179 / 163 / 105 / 44 / 71.

Renders: `docs/screenshots/gfx/cycles_room_final_all.png` (realtime, SSGI + SSR + High soft
shadows), `cycles_room_final_all_baked.png`, `cycles_room_compare.png` (realtime | baked, 1024²
each), `cycles_room_stripes_before.png` / `cycles_room_stripes_fixed.png` (the bug, and the
noise fix alone).

Remaining gap to the path tracer: back wall 10 darker, pillar front +10, cube front +6, floor in
the cube's shadow +13 (direct shadows under the boxes are softer than Cycles'), and the
realtime floor reflection is fainter.
