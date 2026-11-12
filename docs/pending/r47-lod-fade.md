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

## Status (paused 2026-11-12 18:25 at the coordinator's request)

Last commit is `[WIP]`: the tree band moved to *just before* each switch distance (as LOD
Groups), not built or tested yet.

Done and verified (on earlier commits):
- Build, unit tests (lod, lod_group, trees, detail layers, tree shaders), GPU checks
  (`lod_cross_fade_dolly...`: biggest one-step change 0.98 instant vs 0.08 faded, 0 holes;
  `tree_gpu_cull_matches_the_cpu_lists` passes with fades).
- fmt pass; clippy `--workspace --all-targets --release` 0 warnings (before the last two
  commits; re-run).
- Workspace tests (at c1bf8f69): editor 918/1 and engine 1537/1 failing; both failures pass
  alone (`sample_saveloc::saveloc_demo_plays`, `world_partition::...a_big_cell_is_added...`:
  load). Every `*_matches_its_generator` test passes with the Cross Fade sample rocks.
- Timings (round A, main vs branch, same scenes, two interleaved rounds, High 1080p, sum of
  GPU parts): NoonValley hero 13.45 → 13.46 ms, inside 11.78 → 11.97 (trees 1.75 → 1.85),
  MeadowDemo behind 14.81 → 15.01 (trees 0.42 → 0.57), oak 12.62 → 12.93 (trees 0.21 → 0.45).
  Later timing runs were swamped by other GPU load (hero 60-260 ms): ignore them.
- Dolly proof (NoonValley treeline): biggest one-frame step of a switching 32 px block
  41 levels instant → 5.7 faded; strips in `docs/screenshots/r47/`, frames in
  `E:/MistyTargets/r47-shots/` (~0.9 GB, delete when done), tools `tools/lod_dolly_*.py`.

Golden shots — the open problem:
- With fades off (`MISTY_LOD_FADE=0`) the high shots pass: the TAA changes alone are fine.
- Centred tree band (0.1): meadow_high_close 0.45/0.42 %, high_behind 0.68/1.13 %,
  high_oak 0.71/1.39 % DRIFT (limit mean 0.5, changed 0.2 %). The oak in `oak` sits inside
  High's band (lod 30 m × bias 1.5 = 45 m) and is drawn half LOD 0, half LOD 1.
- Band beyond the switch (1d3f8bab): worse (high_oak 1.20/2.20 %, high_close 0.84 %,
  low_behind 0.40 %) → the oak is just beyond 45 m.
- Band before the switch (this WIP): should leave everything beyond each distance as on main;
  not tested yet.

Next:
1. Build (`build_engine.sh`), run `variant.sh v3` (golden + treeline dolly) under the lock.
2. If golden still drifts: narrower default tree band (0.05) or exclude... then explain the
   remaining change (fading trees in a band are a mix of two levels) within tolerance.
3. Re-run clippy, the lod/trees unit tests, `tree_gpu_cull_matches_the_cpu_lists`, golden 20.
4. Final timings when the GPU is quiet (base_engine.exe vs new, interleaved, `times.sh`).
5. Fix the progress.md Round 47 numbers/wording to the final band; drop `[WIP]`.
