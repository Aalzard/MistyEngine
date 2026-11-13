# Round 66 Script Graph fixes: running list

Branch `r66-node-fixes` (worktree `MistyWork\r66-node-fixes`, from main b88e54c9). Cargo target
`E:\MistyTargets\r66-node-fixes` (`CARGO_INCREMENTAL=0`), TMP/TEMP `C:\MistyTry\r66tmp`.

Bugs come from the round 65 game agent building a 2D platformer from Script Graph nodes only
(`docs/pending/r65-node-game.md` on its branch). One fix per issue, each with a regression test.

Status: **fixed** (commit + test), **open**, **checked** (nothing to fix).

## Batch 1

| # | Issue | Status | Fix | Test | Commit |
|---|-------|--------|-----|------|--------|
| B1.1 | 2D triggers and collisions never reached Script Graphs: `dispatch_2d` called Lua only. | fixed | New event nodes named like the Lua callbacks: On Trigger Enter / Stay / Exit 2D (Other) and On Collision Enter / Stay / Exit 2D (Other, Point, Normal, Relative Velocity). The 3D nodes stay 3D only. | `visual_script::tests::trigger_and_collision_2d_events_reach_graphs` | (see git log) |
| B1.2 | UI Button On Click (and On Value Changed / On End Edit, Timeline signals, `ScriptHost::send_message`) only called Lua methods: a graph-only target warned "no script has a method". | fixed | `send_message_with` also runs the target's Custom Events of that name (value as first argument) and counts that as found. Inspector: a ▾ menu next to Method lists the target's Lua functions and Custom Events. | `scripting::ui_api::tests::on_click_and_on_value_changed_run_script_graph_custom_events`, `inspector::call_methods::tests::*` | (see git log) |
| B1.3 | API nodes of variadic Lua functions had no input pins (signature `(...)`). | fixed | Documented signatures in the API table for Physics2D.Raycast / RaycastAll, Rigidbody2D AddForce / MovePosition, Input.GetButton / Down / Up, Tilemap SetTile / GetTile / HasTile / CellToWorld / GetCellCenterWorld. Optional arguments (`[, x]`) are now optional pins (dimmed, left out while unset); before, they were plain pins. `docs/api` regenerated. | `script_graph::tests::variadic_api_functions_get_documented_pins`, `visual_script::tests::optional_api_pins_are_left_out_while_unset` | (see git log) |
