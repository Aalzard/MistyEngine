# Round 29 — instant grass painting (branch r29-grass-paint)

Goal: a dab of Paint Details shows under the brush within a frame or two (< ~50 ms median on
an idle machine), old blades never vanish, settings sliders never blank the field.

## Done (committed)
- Round 29 (merged): edits keep old blades until replaced, coalescing, edit priority, row
  threads for edited chunks, `RenderStats::busy()`, harness `grass_paint_latency`.
- e7c40026: painted chunks switch to 4 × 4 m tiles (`TILE_BIT` keys in `Grass::chunks`),
  `details::scatter_tile` (exactly the chunk's blades rooted in a tile), whole run hidden
  behind until all 16 tiles are up, then a dab re-scatters only its tiles.

## In progress (committed as WIP, built but not yet verified at the time of writing)
- `TileQueue`: tile jobs on their own threads, newest round first, nearest the brush first;
  a tile's waiting job is replaced by a newer one (tiles may go out again while one runs).
- Settings changes: `outdate_all(false)` = camera priority (`brushed` set marks brush/undo
  edits as urgent); species Healthy↔Dry / Brightness moved to a uniform table
  (`GrassParams::species`, WGSL `grass_dry_mix` / `grass_tint`, `species_mark` in
  `shape.z` < 0), removed from `scatter_key`.
- New tests in `grass_paint_tests.rs`: `tile_jobs_waiting_are_replaced`,
  `settings_changes_keep_blades_and_wait_their_turn`, `species_colour_leaves_the_scatter_alone`,
  `species_colour_edits_show_at_once`; `detail_layers::species_colour_is_looked_up_not_baked`;
  `details::tiles_hold_the_chunks_blades`.

## Measurements so far (machine at 100% CPU from other agents)
- Dab to blades at the brush centre (harness): round 29 chunks 1796 ms median (only 11 of
  120 dabs shown); tiles (FIFO rayon) 1408 / 2580 ms median, all 120 shown; settle after
  release 10.1 s → 2.2–3.8 s.

## Next
- Build (use `/d/MistyTargets/build-slot.sh`, `-j 4`), run the tests above, rerun
  `grass_paint_latency` for tiles+queue vs `D:\MistyTargets\r29-scratch\r29.exe`.
- fmt, clippy (0 warnings), full tests, golden_shots under the GPU lock (species colour
  now from the table: check grass shots unchanged).
- docs/progress.md Round 29 section: add tiles, queue, settings, numbers.
