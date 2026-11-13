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

## Batch 2

| # | Issue | Status | Fix | Test | Commit |
|---|-------|--------|-----|------|--------|
| B2.C | `MISTY_WINDOW_SIZE` capture windows opened maximised. eframe restores the geometry saved under the app name even with `persist_window` off, so the user's maximised editor window came back. | fixed | Sized capture runs use their own app name (`Misty Capture Window`), which never saves geometry. | `window_size_tests::misty_window_size_parses_and_uses_its_own_app_name` | (see git log) |
| B2.E | An editor capture with `MISTY_SCRIPTGRAPH` never reached its frame. | open | Needs a capture run to reproduce. | | |
| B2.A | PlayMode tests never ended when the game loaded scenes itself (`Time.time` restarts at 0). | fixed | The runner keeps the test's game time across loads (`M.clock`, summed scaled frame deltas); `yield`, `waitUntil` and the timeout use it. A wall-clock limit (4 × timeout, at least 120 s) stops paused games. | `testing::tests::play_mode_waits_and_timeouts_survive_game_side_scene_loads` | (see git log) |
| B2.4 | No typed 2D physics nodes. | fixed | Native nodes: Rigidbody 2D velocity / angularVelocity get and set, AddForce, MovePosition; Physics2D Raycast (Hit, Point, Normal, Distance, Collider), OverlapCircle, OverlapCircleAll (List), OverlapBox, OverlapPoint. Layer Mask -1 = default layers. | `visual_script::tests::native_2d_physics_nodes_have_typed_pins_and_work` | (see git log) |
| B2.B | 2D friction was averaged: a frictionless player stuck to a 0.4 wall. | fixed | Friction combines as the geometric mean (one frictionless collider slides), bounciness as the maximum, like the familiar 2D engines. The 2D Platformer player (friction 0) no longer sticks. | `physics2d::tests::a_frictionless_body_slides_down_a_wall_it_is_pushed_against` | (see git log) |
| B2.D | No MCP Script Graph tools. | fixed | `graph.create`, `graph.read`, `graph.add_node`, `graph.remove_node`, `graph.connect`, `graph.disconnect`, `graph.set_value`, `graph.add_variable`; `scene.add_component` attaches a `.vgraph`. | `mcp::tests::graph_tools_build_a_script_graph_and_attach_it` | (see git log) |
