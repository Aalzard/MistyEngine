# Round 33 QA: bug list and handoff notes

Branch `r33-qa` (worktree `MistyWork\r33-qa`, from main 431dfc3a). Cargo target
`D:/MistyTargets/r33-qa` (`CARGO_INCREMENTAL=0`, `TMP`/`TEMP` = `D:\MistyTargets\tmp\r33-qa`),
scratch `D:\MistyTargets\r33-scratch\`.

## State at handoff (2026-11-10)

- A baseline `cargo test --workspace --release --no-fail-fast` of main 431dfc3a (before any
  edit) was started in the background; its log is `D:\MistyTargets\r33-scratch\test1.log`
  (it was still compiling `misty_editor` at handoff). It reflects the *unedited* sources: use
  it for the flaky-test baseline and the failure list of main, not to verify the fixes below.
- Commit "WIP: terrain fixes (r33-qa)" holds the first batch of fixes **unbuilt and untested**.
  First thing to do: `cargo test -p misty_engine --release --lib terrain` and
  `cargo test -p misty_editor --release --lib undo_terrain`, then `cargo fmt --all`, fix
  whatever fails, and split the batch into one commit per bug if you have time (git
  `add -p` is not available in this environment; separate commits are fine by file).
- Five read-only review agents covered: terrain, scene/prefab/assets, Lua + Script Graphs,
  tilemap/2D, and MCP/physics/animation/audio/UI/VFX/import/build. Their findings are the
  list below; each was verified against quoted code by the reviewer but not yet reproduced
  by running anything. Severity: C = crash/data loss, W = wrong behaviour, P = perf, X =
  cosmetic.

## Status (2026-11-10, second agent) — RESUME HERE

Everything below "Fixed" is committed but **NOT yet compiled or run** (the baseline test run
held the target; then usage limits). Next steps, in order:

1. `source /d/MistyTargets/build-slot.sh; take_slot r33-qa` (max 3 builds machine-wide,
   `-j 4`, `drop_slot` right after cargo ends, also on failure). Env: `CARGO_TARGET_DIR=
   D:/MistyTargets/r33-qa CARGO_INCREMENTAL=0 TMP=TEMP=D:\MistyTargets	mp33-qa`.
2. `cargo test --release -j 4 -p misty_engine --lib --no-run`, then the editor
   (`-p misty_editor --bin misty --no-run`); fix compile errors (all code below was written
   without a compiler: expect small API mismatches, e.g. mlua `Table::to_pointer`,
   rapier2d `Vector` methods, `Option::is_none_or`).
3. Run the new tests by name (list per commit below), then fmt, clippy, full tests, golden
   (`golden_shots`, GPU lock) — the seam shader change needs the golden check.
4. Red check (optional): crash tests (V1 stack overflow, V3-V7 panics) were not run red.
5. `docs/progress.md` Round 33 section; no Lua API surface changed (no `docs/api` regen
   needed: only error behaviour).

Main was merged in (540d532a, r30 terrain Inspector). F1 and T4 now belong to r33.

Baseline (main 431dfc3a, `D:\MistyTargets33-scratch	est1.log`): editor 811 passed, 3
failed: `registry_url_page_in_project_settings` (still "Loading the registry index…" after
200 frames: timeout), `opening_a_scene_lifts_a_view_left_under_the_terrain` (terrain not
found: project root moved by a parallel test), `string_literal_lint` (caused by this
branch's then-unfinished multi-line Lua literal in visual_script/tests.rs, fixed). The engine
lib run was still going when this was written.

### Done this session (commits on r33-qa, unbuilt)

| Bug | Commit | Fix | New tests |
|-----|--------|-----|-----------|
| V1 C | 22b8fea0 | constant/Tunnel arms of `pull` go through `visiting` | `visual_script::tests::constant_and_tunnel_loops_are_reported` |
| V2 C | 22b8fea0 | `from_lua` path set + depth 64 + 100k item budget | `lua_tables_that_hold_themselves_convert_without_overflowing` |
| V10 | 22b8fea0 | `Op::Incr` saturating (no test: release can't observe) | — |
| V3-V7 C | 2ef27011 | `alive()` checks (made `pub(super)`) in Rigidbody, Rigidbody2D, Collider2D, Tilemap, Terrain, Network Spawn/rpc/Teleport; ragdoll disable tolerates destroyed bones | `rigidbody_of_a_destroyed_object_errors_in_lua`, `rigidbody2d_and_collider2d_of_a_destroyed_object_error_in_lua`, `tilemap_of_a_destroyed_object_errors_in_lua`, `terrain_of_a_destroyed_object_errors_in_lua`, `network_calls_on_destroyed_objects_error_in_lua`, `ragdoll_disable_after_a_bone_was_destroyed` (rpc: no test) |
| V9 | 2ef27011 | `GetContact` `checked_add` | — |
| P1 C | 2ef27011 | `Scene::world_parts` (finite rotation by chaining local rotations when a scale is 0); `world_rotation`, physics 3D/2D `world_pose`, `sync_body`, network teleports use it; collider scale ≥ 1e-3. Not fixed: `set_world_position` under a zero-scale *parent* still inverts a singular matrix | `physics::tests::zero_scale_bodies_keep_finite_poses`, `physics2d::tests::zero_scale_bodies_keep_finite_poses` |
| B1 C | 2ef27011 | `polygon()` filters non-finite/duplicate points, rejects zero area | `polygon_with_nan_or_repeated_points_builds` |
| V8 C | a95f7793 | `snapshots.get()` → nil | `a_deleted_snapshot_has_no_name` |
| W1 C | fbeec148 | `to_json` path set + 1M value budget | `save_api::tests::shared_and_self_holding_tables_save_in_bounded_time` |
| F8 C | 67052217 | index entries must lie in [16, index offset); read caps preallocation by 1032× stored; LZ4 prepended size checked | `pak::tests::corrupt_entry_sizes_are_refused` |
| Atomic C | d3917789 | `asset_db::write_atomic` (`.<name>.<pid>.<n>.tmp` + rename, falls back to in-place write if the rename is refused; no backup file kept) at scene/file, multi_scene, prefab, terrain/file, Material, meta write_guid/write_settings | `asset_db::tests::atomic_writes_replace_whole_files_and_keep_the_old_one_on_failure` |
| F2 C | 3f171576 | `prefab_mode::rename` called from `update_memory` | `prefabs::tests::renaming_the_prefab_in_prefab_mode_follows_the_stage_and_the_stashed_scene` |
| F3 C | d3917789 + 3f171576 | `Prefab::load_with_report`; editor AssetDatabase pushes warnings to `messages` and refuses `save_prefab` for prefabs that lost something on load (except "added default") | `prefabs_with_unknown_components_are_not_written_back` |
| F4 C | 3f171576 | `move_with_meta` rolls the file back | `asset_ops::tests::a_failed_meta_move_rolls_the_rename_back` |
| F5 W | 3f171576 | target = `{stem}.{ext}` | `names_with_dots_keep_the_extension` |
| F1 C | fe857ccf | in Play, `begin_edit` snapshots the terrain into `PlaySession.terrains`, `end_frame` neither saves nor pushes undo, `play::stop` restores | `terrain_tools::tests::edits_during_play_are_reverted_on_stop_and_not_saved` |
| T4 P | — | already fixed by r30: `terrain_previews::counts` caches per version and recounts at most every 300 ms (still on the UI thread while painting; fine) | — |
| Seams W | 0288cf67 | `uv_centroid` (location 7, `perspective, centroid`) set in vs_main, terrain, vgeo mesh/resolve, custom wrap; `fs_unlit` samples it with `textureSampleGrad` using the centre-uv gradients × 2^bias | `render::sprite_seam_gpu_tests::tiles_sharing_an_atlas_edge_show_no_seam_with_msaa` (runtime texture 1024×16 red/blue, 4 tiles of 23 px at x = 10.25) |
| M1 C | 968d8b70 | `read_line` reads through `take(MAX_LINE + 1 - len)` | `mcp::bridge::line_tests::overlong_lines_are_refused_at_the_limit` |

### Flaky tests (fixed in 6b3cc2f7 / 968d8b70, unbuilt)

- `registry_url_page_in_project_settings`: 200 frames ran out while loading under load →
  60 s wall-clock deadline (exits as soon as it is not Loading).
- `new_project_page_shows_the_meadow_demo_card`: 1500 frames × 20 ms for copying the meadow
  project → 300 s deadline.
- `opening_a_scene_lifts_a_view_left_under_the_terrain`: terrain store keys by
  `assets::absolute` (global root) → new `assets::root_generation()`; the test retries (≤20)
  only when the root was moved by another test *and* the lift failed.
- `bridge_writes_and_removes_its_info_file`: passed in baseline; likely Windows sharing
  violation on `remove_file` right after writing (indexer/AV) → Drop retries removal 20×25 ms.
- `shader_graph_demo_renders`: passed in baseline; aligned with the examples fix
  (`shader_asset::reload` before load; material texture maps made absolute).

### Still open

T6, Tex (zero-size image: also `mip_chain` would loop forever on 0×0), F7, F9, F6, Build, W2,
W3, Mixer, Audio NaN, B2-B6, MCP schemas, physics exit events, F10, W6, X items. Not covered:
live-editor MCP checks, player builds, golden check.

## Route to other branches

- (none left: F1/T4 came back to r33 after r30 merged.)

## Fixed in the WIP commit (tests written, NOT yet run)

| # | Sev | Bug | Where | Test |
|---|-----|-----|-------|------|
| T1 | C | Per-texel blade count unbounded: big terrain + coarse control map (e.g. 4000 m, alphamap 16) gives 5e7 blades per texel; `k * 97` overflows u32 (debug panic on the grass worker thread, release stall/GB alloc). Capped with `details::MAX_PER_TEXEL` (262 144) and `wrapping_mul` in `details::scatter_level`, `detail_layers::species_blades`, `flowers::scatter`. | `terrain/details.rs`, `detail_layers.rs`, `flowers.rs` | `terrain::tests::scatter_on_a_huge_texel_is_bounded` |
| T2 | C | `TerrainUndo::restore` could leave `heightmap_resolution`/`alphamap_resolution` inconsistent with map lengths when the asset was reloaded at another resolution outside the history (file replaced on disk → `store::poll_changes`), then any `height()`/mesh build indexes past the end. Now `data.sanitize()` after the swaps (`TerrainData::sanitize` made `pub`). | `misty_editor/src/undo_terrain.rs`, `terrain/data.rs` | `undo_terrain::tests::restore_after_an_outside_resize_keeps_the_maps_consistent` |
| T3 | W | World Partition `tiles::split` copied the *whole* painted flower mask onto every tile (`copy_settings` cloned `flowers` incl. `map`). Now cleared in `copy_settings` and resampled per tile next to `detail_map`. | `world_partition/tiles.rs` | extended `splitting_a_terrain_keeps_its_shape_trees_and_paint` |
| T5 | W | Undo step `bytes()` ignored `alphamap_extra`, `holes`, `flowers.map`, `border`, detail-layer maps and trees kept whole in `settings`, so the 512 MB undo cap never triggered (a Paint Details stroke on a 16-layer 2048 terrain retains ~100 MB reported as 4 KB). Counted now. Proper fix later: diff those maps as `Patch`es too. | `undo_terrain.rs` | `bytes_count_the_maps_kept_whole` |
| T7 | C | `.terrain` file with garbage `heightmap_resolution` (e.g. 4e9) and hole bits: `cells * cells` overflowed / allocated GB before `sanitize`. Holes are now unpacked after `sanitize`, from the snapped resolution. | `terrain/file.rs` | `garbage_resolution_with_holes_loads` (note: it allocates a 4097² heightmap, ~70 MB; fine in release) |

## Open bugs, ranked (not yet fixed)

### Crash / data loss

- **F1** Terrain Inspector edits during Play (Flatten All, Fill Terrain Holes:
  `panels/inspector/terrain.rs:225-238`) go through `begin_edit`/`end_frame` which saves the
  `.terrain` file immediately; Stop restores `undo` from the play session and drops the step,
  so the edit is on disk and not undoable. Only the brush is gated (`terrain_tools.rs:206`).
  Fix: gate `begin_edit` on `!state.play.in_play_mode()` (or snapshot the terrain store in
  `PlaySession`). **Route to r30-terrain-ui** (it owns `inspector/terrain.rs` and
  `terrain_tools.rs`); a small gate inside `begin_edit` is the least conflicting place.
- **V1** Script Graph compiler: `compile.rs:689-694` early-returns for constant nodes
  (`Float/Int/...` pull `Value`) and `Tunnel` run *before* the `visiting` cycle guard, so two
  Float nodes wired `A.Value→B.Value` and `B.Value→A.Value` (accepted by `check_connection`)
  recurse forever: stack overflow in the editor the moment the second wire is made (it
  compiles on every change). Same via two Subgraph nodes of a pass-through macro. Fix: move
  the `visiting` insert/remove around those arms (or reject data cycles in
  `check_connection`). Test next to the AddFloat loop test in `visual_script/tests.rs` ~1118.
- **V2** `visual_script/lua_bridge.rs:61-71` `from_lua` recurses without depth/cycle limit:
  a Lua function returning `t[1] = t` into a graph overflows the stack. Fix: depth param
  (cap 64) like `save_api::to_json`.
- **V3–V7** Lua handles used after `Destroy(go)` hit `Scene::game_object/transform`
  `expect`s (`scene/mod.rs:210-234`) and abort the process (mlua re-raises Rust panics):
  `Rigidbody.position/MovePosition/MoveRotation` (`scripting/api.rs:578-622`),
  `Rigidbody2D.position` and `Collider2D:CompareTag` (`physics2d_api.rs:88-93,227`),
  `Tilemap:WorldToCell/CellToWorld/GetCellCenterWorld` (`tilemap_api.rs:89-111`),
  `Terrain:GetPosition` (`terrain_api.rs:59`), `Network.Spawn(go,pos)`, rpc call,
  `NetworkTransform:Teleport` (`network_api.rs:661,969,1231`), `Ragdoll.Disable` after a
  bone was destroyed (`physics/ragdoll.rs:620`). Fix: `if !w.scene.contains(e) { return
  Err(missing()) }` (helper `alive` exists at `scripting/api.rs:140`). One test per API in
  the matching `*_tests.rs`: keep the handle, Destroy, update twice, call, assert error log.
- **V8** `audio_api.rs:348` `m.asset.snapshots[s.1]` unchecked index (snapshot deleted in
  the Mixer panel during Play). Use `.get()`.
- **V9/V10** debug overflow: `physics2d_api.rs:544` `GetContact(i)` `i + 1`
  (`checked_add`); `vm.rs:257` `Op::Incr` on `i64::MAX` (For loop `Last = i64::MAX`), use
  `saturating_add`.
- **W1** `save_api.rs:283` `to_json` has a depth cap but no visited set: a table referenced
  twice at each level (`t.a = t; t.b = t`) costs 2^64 in `OnSave` → editor freeze. Add a
  `HashSet<*const c_void>` of tables on the path (`Table::to_pointer()`).
- **P1** Zero world scale on any axis makes rigid-body/collider poses NaN (3D
  `physics/mod.rs:128-131`, 2D `physics2d/mod.rs:78-81`: `to_scale_rotation_translation` of a
  degenerate matrix), then `sync_out` writes NaN into Transforms. Fix: take rotation from
  `scene.world_rotation(e)` and scale from the column lengths clamped to `1e-3`. Test:
  cube scale (1,0,1) + Rigidbody, build + step, assert finite pose.
- **B1** `physics2d/colliders.rs:192-202` `PolygonCollider2D` with a NaN point:
  `is_convex` false → `convex_decomposition` → parry `Compound::new` asserts on zero hulls.
  Filter non-finite/duplicate points first, warn if < 3 remain.
- **F8** `pak.rs:223,234` `vec![0u8; e.stored]` / `with_capacity(e.size)` straight from the
  index: a corrupt pak aborts on allocation. Validate `offset + stored <= index offset`.
- **Atomic saves** (data loss on crash/disk full): every asset save is a bare
  `std::fs::write`. Add `asset_db::write_atomic(path, bytes)` (write `<name>.tmp` next to
  it, rename over; `save/mod.rs:247` already does this for game saves) and use it at least
  in `scene/file.rs:439`, `scene/multi_scene.rs:424`, `scene/prefab/mod.rs:94`,
  `terrain/file.rs:183`, `assets.rs:303` (Material), `asset_db/meta.rs:84,96`. Watch out:
  `store::save` and `AssetDatabase::save_prefab` read the mtime *after* the save, which is
  still right after a rename. Check the Project window / file watcher ignore a transient
  `.tmp` (name it `.<file>.tmp`? `valid_name` rejects leading dots for user names only).
- **F2** Asset rename/move while Prefab Mode is open: `asset_actions.rs:292-306`
  `update_memory` skips `prefab_stages[*].stash.scene/undo` and `PrefabStage.path`
  (renaming the edited prefab makes `save()` recreate the old file). Fix: remap those too,
  or refuse rename/move while `prefab_mode::active`.
- **F3** `Prefab::load` drops the `LoadReport` (`scene/prefab/mod.rs:98-104`): a prefab with
  an unknown component loads silently truncated and `prefabs::propagate` then removes that
  component from every instance and saves. Surface the warnings; consider refusing to resync
  when the asset loaded with unknown components. Also reject forward `parent` indices.
- **F4** `asset_ops.rs:85-104` `rename`/`move_into`: file renamed, then `move_meta` fails →
  Err returned after the move, DB not updated. Roll the rename back on meta failure.
- **M1** MCP bridge `mcp/bridge.rs:201-234`: `MAX_LINE` only checked on timeout; a client
  streaming bytes without `\n` grows the buffer without limit (loopback only). Read through
  `take(MAX_LINE + 1)`.
- **Tex** `render/texture.rs:84-91` `load_image` has no zero-dimension guard before
  `create_texture_with_data` (unverified any decoder yields 0×0; cheap guard).

### Wrong behaviour

- **Seams (known issue, root cause found)** Vertical seams between stone-wall tile columns in
  the 2D Platformer template: geometry/UVs are exact and the sampler is Nearest/Clamp/no
  mips, so it is MSAA centre interpolation: for an edge pixel partially covered by both
  quads, the left quad's fragment evaluates `uv` at the pixel centre, slightly past u = 0.5,
  which is the atlas boundary to `Props_1` (the torch) whose first column is transparent
  (`templates/platformer_2d/art.rs:196-215`, `Props.png` 32×16 sliced 16×16). Top/bottom and
  the right quad clamp to the texture edge, hence vertical only, wall only. Fix in
  `render/shaders/mesh.wgsl`: add `@location(7) @interpolate(perspective, centroid)
  uv_centroid` to `MeshOut`, set it in `vs_main`, and sample with it in `fs_unlit` (line
  341) only (keep `uv` centre-interpolated for lit materials so mip derivatives don't jitter).
  Check `terrain.wgsl` fills the new field (it builds a `MeshOut` by hand, `vs_terrain`
  line 11). Then run the golden check. Regression test idea: GPU test in
  `render/gpu_tests.rs` with a 32×16 texture (left half red, right half transparent) sliced
  16×16, a 4×1 tilemap, ortho camera with a 23 px cell + 0.37 px offset, MSAA 4: every pixel
  inside the map must be red. Belt and braces: a 1 px gutter in the generated sheets.
- **F7** `scene_files.rs:540` sets the scene path before `save_scene` succeeds; a failed
  Save As leaves the scene pointing at a missing file and the next Ctrl+S writes there.
  Save first, then `set_scene_path`.
- **F5** `asset_ops.rs:73-78` rename with a dot in the new name: `Red.v2` → `set_extension`
  → `Red.mat` == from → silently no-op; `Red.v2.x` → `Red.v2.mat`. Build the target as
  `format!("{new_stem}.{ext}")`.
- **F9** `Scene::save` / `to_json` (`scene/file.rs:417-441`) write *every* loaded scene's
  objects into one file (the editor uses `save_scene(id)`, but MCP/editor scripts/samples may
  call `Scene::save` with additive scenes loaded). Make it save the active scene only.
- **F6** Save As / Open path checks `starts_with(project.root…)` fail when the root is not
  canonical (`./SampleProject/.`, `\\?\`, drive-letter case). Canonicalize the root once.
- **Build** `build_pipeline/package.rs:186-195`: a missing enabled build scene is only a
  warning; the build "succeeds" and the player fails at `load_scene`. Return Err.
- **W2** Script-supplied paths joined with `Path::join` (`scripting/api.rs:977` prefab,
  `audio/mod.rs:208`, `input/actions/system.rs:226`, `visual_script/runtime.rs:191`,
  `visual_script/mod.rs:208`, and `network_api.rs:2117` from a *remote* SpawnMsg) accept
  absolute paths and `..`. One `project_path(root, rel)` helper rejecting absolute/Prefix/
  ParentDir components.
- **W3** `ParticleSystem:Emit(4294967295)` spins 4.3G iterations (`particles/sim.rs:390`);
  clamp `n` to the free capacity.
- **Mixer** `audio/mixer.rs:173-179` sanitize doesn't break parent cycles (A↔B): the groups
  vanish from the Mixer window. Walk parents with a visited set.
- **Audio NaN** `audio/mod.rs:372` `volume.clamp` passes NaN; use `is_finite` fallback.
- **B2** Sprite-type tile colliders ignore the tile's rotation/flip (`tilemap/collider.rs:37`);
  use `chunks::quad_corners` AABB. **B3** tilemap compound colliders could use parry's
  `CompoundFlags::FIX_INTERNAL_EDGES` to stop bodies catching on seams between rects.
- **B4** `sprites/slice.rs:298-300` u32 overflow with absurd padding (debug); `checked_add`.
- **B5** `tilemap/mod.rs:95` cells saturate to ±i32::MAX, then `edit.rs:435-467` overflow /
  `box_fill` loops over 2^64 cells on a drag far outside a tiny-cell grid. Clamp cells to
  ±(1 << 20) and cap box areas.
- **B6** tilemap chunk cache keyed by entity bits only (`chunks.rs:267`): two scenes with
  the same entity ids thrash rebuilds every frame. Add a scene id to the key.
- **MCP schemas** advertise `minimum/maximum/items` that `validate` (`mcp/tools/mod.rs:171`)
  doesn't enforce (no panic; `docs/mcp.md:109` overstates). **asset.read_text** reads the
  whole file before truncating (`mcp/tools/asset.rs:195`).
- **Physics exit events** for a destroyed object are never delivered (`physics/mod.rs:381-394`
  filters through `self.colliders` after `remove`); may be intended, document or emit
  `started: false` before removing.
- **F10** asset watcher `poll_in` vs editor duplicate: a scan between `copy_all` and
  `copy_meta` can reassign the *original's* GUID on file systems without `created()`.
  Prefer the DB's existing path on GUID collision.
- **W6** (unverified by me) `rebind:WithTargetBinding(i)` skips validation; `Network.
  StartClient("host:7777", 7777)` → `host:7777:7777`; UI/director/video listeners keyed by
  entity bits never removed on destroy (memory growth); `1i64 << area` in
  `navigation_api.rs:91,427` if area ≥ 64.

### Perf / cosmetic

- **T4 (P)** Paint Details tab calls `detail_layers::counts(d)` every frame
  (`terrain_details_ui.rs:68`): full scatter of every mesh layer + n² sums at 2048. Cache per
  store version. **Route to r30-terrain-ui** (owns `terrain_details_ui.rs`).
- **T6 (P)** `render/terrain.rs:1069-1078` hole index lists rebuilt wholesale whenever any
  holed chunk's LOD mask changes.
- **X** `SaveSystem.version` wraps at 2^32 (`save_api.rs:106`); `.vgraph` `"id":
  4294967295` overflows `next_id` (`visual_script/mod.rs:271`, `compile.rs:427`); duplicate
  top-level node ids fire events twice; VM `Exit::Error` for `Op::Send` reports node 0;
  `fov.clamp` passes NaN; `name_from_path` for dotfiles gives ""; `rewrite_text` is
  case-sensitive on Windows; Play exit doesn't restore `inspector_targets`/`hierarchy.expanded`.
- **Not bugs, noted by reviewers as correct**: resolution/size changes keep every terrain map
  consistent; LOD chunk math for 33..4097; GPU texture sizes within limits; collider and mesh
  share the diagonal split; Region::cells clamps; MCP arg handling (`project_path` rejects
  absolute/`..`); scene load parent-index handling; prefab diff/sync indexing; multi-scene
  load-before-mutate; physics mass/joint/raycast guards; animation 0/1-key curves; naga
  validation of user WGSL; VM op budget and depth caps.

## Flaky tests (still to investigate)

Named flaky under load: `mcp::tests::bridge_writes_and_removes_its_info_file`
(`misty_editor/src/mcp/tests.rs:1000`), `panels::packages::tests::
registry_url_page_in_project_settings` (`panels/packages.rs:563`), `scene_files::tests::
opening_a_scene_lifts_a_view_left_under_the_terrain` (`scene_files.rs:700`),
`new_project_page_shows_the_meadow_demo_card` (`hub/tests.rs:306`), `shader_graph_demo_renders`
(`sample_shader_graph.rs:561`). Suspects to check first: shared temp dir names built from
`process::id()` only (two tests in the same process using the same folder; the terrain tests
use `misty_terrain_{name}_{pid}` which is fine, check these five), global state
(`terrain::store`, `assets::absolute` project root moved by other tests, `file_watch`),
timing (`bridge` info file written by a thread; `recv_timeout`), and GPU contention for the
shader graph render (compare with the earlier fix for `shader_graph_examples_render`). Use
the baseline log `test1.log` to see which of them failed on this machine.

## Not yet covered

- Running anything in the live editor through the MCP bridge on a temp project copy under
  `D:\MistyTargets\r33-scratch\` (terrain create/resize/brushes/16 layers/undo/save/reload,
  Play enter/exit, multi-scene, Project window rename/drag-drop).
- Player builds with terrain; dedicated server; build profiles.
- Golden check (`golden_shots`, GPU lock protocol in the task brief) after the shader change.
- `docs/progress.md` Round 33 section; `docs/api` regeneration if Lua bindings change.
