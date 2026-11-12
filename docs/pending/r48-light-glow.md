# r48-light-glow — Light ▸ Glow: soft glow spread per light

Branch `r48-light-glow` (worktree `MistyWork\r48-light-glow`, from main 16543191).

## The report

Raising a light's intensity (the Rectangle light especially) left the light itself fine but
surrounded it with a harsh, hot orange glow with a fairly hard edge. Wanted: an option that
spreads the glow out softly.

## What causes it

Headless study (`render::glow::spread_tests::glow_spread_shots`, 640×360, dark floor, a
1.2 × 0.8 m Rectangle light with its emitter and a point light with a Halo, both warm
orange at intensity 60; shots in `E:\MistyTargets\r48-shots\study_*.png`):

| shot | what shows |
|------|------------|
| `study_a_default` | the user's look: a hot blob around the panel, a big orange glow with a visible round edge |
| `study_b_no_bloom` | bloom off: a crisp panel, the halo sprite, no glow around the panel at all |
| `study_c_no_halo` | halo off: the panel's glow is unchanged |
| `study_d_fog`, `study_e_fog_no_bloom`, `study_f_fog_no_bloom_no_halo` | volumetric fog: in-scattering is smooth, with no hot core and no edge |

So the glow is **bloom of the emitter quad** (and of the halo sprite, which also feeds the
bloom buffer). Bloom adds every level of its 6-level chain at weight `scatter^k`, which is 1
at the default Scatter 0.7: the bloom image carries 0.6 × 6 = 3.6 times the emitter's
energy, the narrow levels pile up into a blob about as bright as the emitter right around
it, and the tail ends where the widest level (1/64 resolution) ends: the hard edge. The
volume's Bloom Scatter (0..1, exists already) only reweights those levels; it cannot widen
past the chain or cool one light without changing every other.

## What was added

**Light ▸ Glow** (point, spot and Rectangle lights; folded away while untouched):

- **Intensity** (default 1): how strongly the light's visible shape glows, independent of
  how much it lights the scene. Rectangle: scales what its emitter feeds into bloom.
  Point / spot: scales the Halo sprite.
- **Spread** (0..1, default 0): moves that share of the light's bloom into a halo of its own,
  three Gaussians (σ = 0.04 / 0.12 / 0.30 screen heights × (0.6 + 0.8 · Spread), energy
  25 / 35 / 40 %) of the screen distance to the projected emitter quad (a rounded-rectangle
  glow) or around a halo's centre. The halo carries exactly the energy taken out of bloom
  (Steiner: a Gaussian of distance around a convex shape integrates to
  a · (area + perimeter · σ · √(π/2) + 2π σ²)), using bloom's real gain
  (Intensity × Σ scatter^k over the level count, plus its threshold, clamp and tint). So the
  core cools down and the tail widens with no extra brightness.
- **Soft Core** (0..1, default 0): Rectangle lights compress the emitter's visible highlights
  (`e / (1 + Soft Core · max(e) / 4)`, hue kept) while its glow keeps the uncompressed
  emission; point / spot lights dim the Halo sprite's hot centre (energy moved to the rest of
  it).

The spread halo is drawn in the glow pass (`glow.wgsl` `vs_spread` / `fs_spread`) into the HDR
image only, with the halos' depth-tap occlusion and fog. Emitter quads get the glow share and
Soft Core through the free `lightmap.zw` of their draw uniform; `mesh.wgsl`
`light_glow_surface` applies them (0, 0 = unchanged).

Defaults change nothing: `LightGlow` is left out of scene files while untouched
(`skip_serializing_if`), old files load with the defaults, the default halo / emitter
numbers are bit-identical (tests), and no spread instance is drawn.

Not changed: the volume's Bloom settings (its Scatter is already a 0..1 spread of the whole
chain); the per-light Spread is the tool for one harsh light.

## Shots (`E:\MistyTargets\r48-shots`)

Same dark scene. Intensity 15 (the glow not fully clipped): `i15_spread_0` (before),
`i15_spread_05`, `i15_spread_1`, `i15_spread_05_soft_core`. Intensity 60 (the user's case):
`spread_0` (before), `spread_05`, `spread_1`, `spread_05_soft_core`, `glow_intensity_025`;
with fog `fog_spread_0`, `fog_spread_05`, `fog_spread_1`. Energy test images
`energy_spread_{0,5,10}`.

## Measured

Energy test (`glow_spread_keeps_the_energy_and_cools_the_core`, a white panel of radiance
0.3 on black, tonemapper None so the image is linear): glow energy summed over the screen,
glow just outside the panel's edge, and 80-100 px out:

| spread | glow energy | near edge | tail |
|--------|-------------|-----------|------|
| 0 | 2153 | 0.157 | 0.0014 |
| 0.5 | 1965 (91 %) | 0.140 | 0.0040 |
| 1 | 1580 (73 %) | 0.085 | 0.0076 |

The full-plane integral matches to 3 % (CPU test `spread_weights_carry_the_requested_energy`);
on screen spread 1 loses its widest Gaussian's edges past the frame and its faintest tail to
8-bit output.

GPU cost (`glow_spread_benchmark`, 1080p, RTX 4060, 8 Rectangle lights with emitters + 8
haloed point lights, whole frame, two passes each): spread 0 1.00 / 0.94 ms, spread 1
2.15 / 2.01 ms: about 0.07 ms per light whose spread halo covers the whole screen (measured
before the "faint reach" trim below, which only shrinks the quads of dim lights).

## Tests

- `scene::light_glow`: untouched glow is not saved (default, Rectangle, Directional lights),
  a touched one round-trips, partial objects load the other defaults, old lights load and
  re-save without new keys.
- `render::glow::spread`: weights carry the requested energy (numeric integral around
  rectangles of several sizes), wider spread lowers the peak, bloom gain follows the
  upsample chain, defaults change nothing, Soft Core keeps the halo energy, the halo energy
  formula matches a numeric integral.
- `render::glow::spread_tests` (GPU): energy and core/tail above; defaults draw no spread
  halo and render identically.

## Status

Done. Final checks on the last commit: `cargo fmt --all --check` clean, clippy
(`--workspace --all-targets --release`) 0 warnings, glow GPU tests and the energy test pass
(after the faint-reach trim: spread 0.5 → 91 %, spread 1 → 72 % of spread 0's on-screen
energy; near edge 0.157 → 0.139 → 0.084; tail 0.0014 → 0.0039 → 0.0075), golden shots 20/20
unchanged. Full workspace tests passed on the feature commit (919 + 1539 + small suites,
0 failed); the later change only trims faint halo quads.

A benchmark rerun on 2026-11-13 was noisy (5-7 ms frames with other jobs on the GPU); the
numbers above are from the quiet run.

Not done: an Inspector look at the Glow section in a live editor.
