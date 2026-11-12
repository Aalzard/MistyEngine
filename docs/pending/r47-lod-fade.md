# Smooth LOD transitions (branch r47-lod-fade)

Dithered cross-fades wherever a level of detail switches: LOD Groups, terrain trees (full
mesh / lighter mesh / impostor / draw distance), detail meshes at their draw distance, and
the shadow casters in step with the camera. Forked from main 16543191.

Disks: target `E:/MistyTargets/r47-lod-fade` (baseline engine test binary from main kept as
`base_engine.exe` there), TMP `C:\MistyTry\r47tmp`, shots `E:/MistyTargets/r47-shots`, scripts
in the session scratchpad `r47/` (`env.sh`, `build_engine.sh`).

## How it works

- **One dither** for every cross-fade: `lod_dithered_out` / `lod_dither` in
  `render/shaders/common.wgsl` (moved out of draw.wgsl, which compute shaders include without
  the view). A 4 × 4 Bayer matrix; while TAA runs every threshold steps by 1/8 per jitter
  phase (8 per cycle; shifting the pattern in space kept a 50 % checker in place, since every
  shift the jitter gives is odd), so the history averages the two levels instead of keeping a
  fixed screen pattern. Fade value `k > 0` keeps thresholds below `k` (outgoing level),
  `k - 1` keeps the rest (incoming level): together exactly one draw per pixel, no blending,
  no sorting. Mirrored in Rust (`render/lod.rs` `BAYER4`, `dither`, `dithered_out`); a test
  checks the shader's matrix is the tested one.
- **LOD Groups** (`scene/lod_group.rs`, `render/lod.rs`, `draw_cache.rs`): the band is
  `fade_width` × each threshold above it (screen height ∝ 1 / distance, so 0.1 ≈ the last
  tenth of the switch distance). Inside it both levels draw; fading opaque draws use the
  cutout variant (only they pay for a discard). New groups default to **Cross Fade**, width
  0.1; a file without `fade_mode` loads as **None** (field-level serde default), so old
  scenes keep their look. A LOD Group whose LOD 0 draws as virtual geometry now also fades
  out above its cull height (the fading draw leaves the cluster path for the band).
- **Shadows**: mesh draws take the camera's LOD state and fade value into the shadow
  casters (already shared, now with the moving dither), so casters switch with what the
  camera sees. Terrain trees keep their per-cascade level (by texel size, round 26): their
  shadow never depends on the camera's switch, and the cascades blend across borders, so a
  camera LOD switch does not change the shadow at all.
- **Terrain trees** (`render/trees.rs`, `trees_cull.wgsl`, `terrain/trees.rs`): new Tree
  Rendering ▸ **LOD Fade** (default 0.1 of each distance, in a band just beyond it, so the
  finer level stays whole up to the set distance; written to the
  terrain file only when changed, so files stay byte-identical). Full mesh → lighter mesh →
  impostor, and the impostor fades out at the draw distance. CPU lists (`levels_at`) and the
  GPU cull (`fade_at` / `append_pair`: a tree in a band goes into both levels' lists) do the
  same; the band reaches the shader in `Params.occlusion.w`. The old interleaved-noise
  `tree_noise` is gone (it read as a hatch pattern around the crowns).
- **Detail meshes** (`terrain/detail_layers.rs`): thin out over the last tenth of their draw
  distance (`DETAIL_FADE`) instead of popping out.
- **Sample rocks** (prefabs, NoonValley, MeadowDemo, StreamingDemo cells): their LOD Groups
  switch to Cross Fade (the generator's rock prefab takes the new default).
- **Off switch** for before / after runs: `render::lod::set_fades(false)` or
  `MISTY_LOD_FADE=0` (every switch instant, as before).

## Editor

- LOD Group Properties: Fade Mode (None / Cross Fade) and Fade Width now go through `tr!()`
  with tooltips; Turkish and Arabic entries in `[inspector]`.
- Terrain ▸ Tree Rendering: **LOD Fade** slider (0–0.3) under Billboard Start; `[terrain]`
  entries.

## Tests

- `render::lod`: `bands_select_one_or_two_levels_and_cover_every_pixel_once` (selection with
  bands), `complementary_dithers_sum_to_full_coverage`, `shader_dither_matches_the_tested_one`,
  `distance_fades_are_centred_on_the_switch`, virtual LOD fade.
- `scene::lod_group`: `new_groups_cross_fade_and_old_files_keep_switching_instantly` (old
  scenes), `fade_settings_round_trip_through_a_saved_scene`.
- `terrain::trees`: `lod_fade_loads_from_old_files_and_round_trips`.
- GPU: `gpu_checks::lod_cross_fade_dolly_changes_gradually_and_covers_the_object` (dolly
  across a switch: gradual colour change, silhouette always covered);
  `tree_gpu_cull_matches_the_cpu_lists` now covers the fades too.
- Harness (ignored): `noon_valley_tests::lod_fade_dolly` — camera dolly, fades off / on,
  pop pixels from the per-pixel second difference; `MISTY_NOON_GRASS=treefade:x`.

## Status (2026-11-13): done, mergeable

- Tree band: just *before* each switch distance (the switch is complete at it, as a LOD
  Group's band). New terrains get LOD Fade 0.1; terrains saved without the field load with 0
  (instant, as before). NoonValley is switched on in its terrain file; MeadowDemo is left as
  saved: its golden views have trees standing right on a switch (`golden_tree_bands`, an
  ignored probe in golden_tests.rs, prints them: e.g. an oak at 0.995–1.0 × 45 m on High), and
  any band, before, around or after the switch, redrew one of them half-and-half (golden
  drift up to mean 1.2 / 2.4 %). Turning it on there needs new golden images (user's call).
- Golden shots 20/20 within tolerance (largest mean 0.39, changed 0.03 %).
- Dolly (NoonValley treeline, final build): biggest one-frame step of a switching block 40.9
  → 4.3 levels; strips in `docs/screenshots/r47/` regenerated from the final build; scratch
  frames deleted.
- fmt pass, clippy 0 warnings, unit / GPU tests and timings: see the Round 47 section in
  docs/progress.md.
