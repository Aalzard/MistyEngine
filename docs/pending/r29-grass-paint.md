# Round 29 — instant grass painting (branch r29-grass-paint)

Goal: a dab of Paint Details shows under the brush within a frame or two (< ~50 ms median on
an idle machine), old blades never vanish, settings sliders never blank the field.

## Done (committed)
- Round 29 (merged): edits keep old blades until replaced, coalescing, edit priority, row
  threads for edited chunks, `RenderStats::busy()`, harness `grass_paint_latency`.
- e7c40026: painted chunks switch to 4 × 4 m tiles (`TILE_BIT` keys in `Grass::chunks`),
  `details::scatter_tile` (exactly the chunk's blades rooted in a tile), whole run hidden
  behind until all 16 tiles are up, then a dab re-scatters only its tiles.

## In progress (committed; tests pass except as noted)
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

## Measurements so far (machine busy with other agents)
- 1fc66dc8 (tiles + queue, switching tiles first), CPU at ~74 %, dab to blades drawn at the
  brush centre, median / 90 %:
  - brush 0.25 m per frame (about 15 m/s at 60 fps): 76 / 228 ms (round 29: 865 / 1082 ms,
    only 61 of 120 dabs ever shown); after release 0.29 s.
  - brush 1 m per frame (60 m/s): 377 / 1756 ms (round 29: 666 / 906 ms, 63 of 120 shown).
- At 55% load: round 29 327 ms median (78 of 120 dabs ever shown); tiles + queue (2310 ms
  stroke) 1020–1040 ms median, all shown; settle after release 0.4 s both. Suspected cause:
  a chunk switching to tiles hides its tiles until all 16 are up, and those tiles lost to
  newer rounds in the queue: 1fc66dc8 puts them first (`Job::first`). Not measured yet.
- Earlier, at 100% load:
- Dab to blades at the brush centre (harness): round 29 chunks 1796 ms median (only 11 of
  120 dabs shown); tiles (FIFO rayon) 1408 / 2580 ms median, all 120 shown; settle after
  release 10.1 s → 2.2–3.8 s.

## Checks (status)
- grass/terrain unit + GPU tests: pass (paint_tests, detail_layers, details).
- `cargo fmt --all --check`: clean.
- clippy: misty_engine clean (cached, no warnings); the workspace run keeps dying in rustc
  with STATUS_ACCESS_VIOLATION on a different dependency each time (eframe, egui-winit,
  webbrowser) — the machine (low virtual memory), not the code. Retry in a build slot.
- golden_shots on the tile build: pass (all shots ok, 0.00–0.03 % changed).
- full workspace tests: not run yet on the tile build ( binary `D:/MistyTargets/r29-scratch/tiles3.exe` = commit 1fc66dc8 + the
  stroke-speed knob).

## Next
- Retry clippy and full tests in a slot; run golden_shots (`tiles3.exe golden_shots
  --ignored --nocapture --test-threads=1` from crates/misty_engine, under the GPU lock).
- Optional: switch the chunks around the brush to tiles ahead of it (fast strokes into
  unpainted chunks wait for a chunk's 16 tiles).
