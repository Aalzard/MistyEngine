# Round 22 — visual scripting (branch r22-visual-scripting)

Node-based game logic like Unreal Blueprints / Unity Visual Scripting: Script Graph assets, a
bytecode runtime in Rust, the Script Graph window with live debugging, and a sample made only of
graphs. User guide: [docs/visual-scripting.md](../visual-scripting.md).

## Done

### Asset and runtime (`misty_engine::visual_script`)
- `.vgraph` JSON assets: nodes (type, settings, inline values), wires, Blackboard variables
  (Graph or Object scope), groups, sticky notes, macros. Round-trips byte for byte.
- A graph runs on an object through its Scripts list (path ends in `.vgraph`), so scenes,
  prefabs, the Inspector, Add Component, drag onto the Hierarchy and builds treat it like a
  script. The Inspector shows *Name (Script Graph)* with the Object variables as fields.
- Flow-based execution: white exec wires plus typed data wires (Bool, Int, Float, String,
  Vector3, Quaternion, GameObject, List, Any); pure nodes are lazy (pulled by the action node
  that reads them, computed once per run of it).
- No Lua is generated: `compile` checks the graph and emits bytecode over a register file per
  instance (constants, outputs, node state); common float math runs as direct ops; Sequence and
  loops use a call stack; Delay schedules a continuation. The VM yields to the host for Lua calls
  and breakpoints. Guards: While Max Iterations, a 20 M step budget per flow, nesting depth.
- Events: Start, Update (Delta Time), FixedUpdate, LateUpdate, Enable, Disable, Trigger
  Enter/Exit and Collision Enter/Exit (Other), Input Action performed / held / released (the
  Input System's actions), Custom Event (named, with arguments) with Send / Broadcast, Timer
  (Interval, Repeat), Destroy.
- Nodes: Branch, Sequence, For Loop (+Break), For Each, While, Do Once, Gate, Flip Flop, Delay;
  ~90 math / logic / comparison / vector / quaternion / random / string / list nodes; constants;
  Get / Set variable, Get / Set Object Variable by name, Self.
- Engine API nodes generated from the Lua binding table (the API reference's), Get / Set per
  property and a node per function; Transform, GameObject, Rigidbody velocity / AddForce,
  Physics.Raycast, Time, Input, Debug.Log and Find / Destroy run natively with typed pins, the
  rest call the Lua binding with values converted both ways.
- Lua interop: Call Lua Function (another script's function, with its result), Get Lua Field,
  custom events delivered to Lua functions of the same name; Lua gets
  `gameObject:SendCustomEvent(name, ...)`, `GetVariable(name)`, `SetVariable(name, value)`
  (API reference regenerated).
- Subgraphs and macros: Graph Input / Graph Output pins (exec and data), a Subgraph node per
  macro or `.vgraph` asset, inlined at compile time with private variables per use; recursion
  refused.
- Errors: compile problems per node (unknown type, missing variable, type mismatch, data cycles,
  For Each without a list, missing / recursive subgraph…); run-time errors logged once per node
  with object, graph and node names.

### Editor: Script Graph window
- Create ▸ Visual Script Graph, double-click a `.vgraph`, Window ▸ Visual Scripting ▸ Script
  Graph; Save Asset, undo / redo, Show In Project.
- Canvas built on the shared node-canvas pieces (new `panels/node_canvas.rs`: view mapping,
  grid, wires, pan / zoom, error badge — now also used by the Shader Graph and Effect Graph
  windows): nodes with exec triangles and typed pins, inline values, groups, sticky notes,
  rubber band, copy / cut / paste (system clipboard as JSON), duplicate, Collapse To Macro,
  double-click into macros with a path bar.
- Right-click / Space search by category (Engine sub-folders per Lua type) with text filter;
  a wire dropped on empty canvas opens it filtered to fitting nodes and wires the new node.
- Type-checked wiring with a refusal message; red badges with the problem on nodes; the toolbar
  and Graph Inspector list problems.
- Blackboard (variables with type, scope, value; Get / Set buttons; live values while playing)
  and Graph Inspector (node settings).
- Live debugging in Play mode: glowing exec wires with a moving dot, values on pin hover,
  breakpoints (F9) that pause Play before the node, Step (F10) and Continue; the selected object
  picks the instance shown.
- Capture options: `MISTY_SCRIPTGRAPH`, `MISTY_GRAPH_BREAK`, `MISTY_GRAPH_SELECT`,
  `MISTY_GRAPH_SEARCH`.

### Sample
- `VisualScriptingDemo` (generator `sample_visual_scripting.rs`, `matches_its_generator` and a
  headless play test): a door that opens on its trigger, coins with a score UI (formatted by a
  macro), a patrol on a timer that respawns the player, a jump pad, and a graph-driven player
  (Character Controller, axes, jump, gravity). No Lua.

### Tests
- Engine (`visual_script::tests`): events (Start / Update / Fixed / Late, timers, triggers and
  collisions, input actions, custom events both ways with Lua, OnDestroy), math / logic / vector
  / quaternion / random / list nodes, every flow node, variables (graph, object, Inspector
  values, Lua), engine API native and through Lua, Call Lua Function, subgraphs and macros,
  compile and run-time errors, save / load, breakpoints and stepping, the benchmark.
- Editor (`panels::script_graph::tests`): create and open, search and create, filtered search
  from a dropped wire, type-checked wiring and badges, copy / paste and macros, Lua API coverage,
  Inspector fields, a breakpoint pausing Play with Step / Continue; the sample's generator and
  headless play.

## Performance
Benchmark test `a_hundred_node_graph_on_a_thousand_objects_runs_in_about_a_millisecond`
(release build, 1,000 objects each running a 100-node graph on Update: ten Set Variable stages
fed by eight pure math nodes each, then moving the object), `--test-threads=1`:

| | median | best |
|---|---|---|
| Graph Update pass (dispatch to 1,000 instances + all flows) | 0.97 ms | 0.91 ms |
| Whole frame minus the same scene without graphs | 1.03 ms | 0.92 ms |
| 1,000 objects with a one-node Update graph (frame) | 0.56 ms | 0.53 ms |
| 1,000 objects with a Start-only graph (frame) | 0.51 ms | 0.42 ms |
| 1,000 objects, no graphs (frame) | 0.42 ms | 0.41 ms |

About 0.1 µs of per-object dispatch and under 10 ns per node (ten `Sin` calls and a Transform
write included). What got it there: the common float nodes run as inline ops (no function
pointers or register lists), Graph variables live in registers (Get Variable costs nothing),
the VM loop has no debugger checks unless the graph is being debugged, the Update pass borrows
the world once per object, and started graphs without enable events skip the per-frame enable
check. The machine was also running other builds, so numbers vary by ±30 %; the test asserts
the pass stays under 4 ms because tests run side by side.

## Screenshots
SCREENSHOTS_PLACEHOLDER

## Not done yet
- Graphs edited while playing apply on the next Play (no hot reload of running instances).
- Save games don't store graph state (Object variables included).
- The debugger shows top-level nodes only (a breakpoint on a Subgraph node stops when the flow
  enters it; nodes inside a macro aren't debuggable while playing).
- Lua-backed API nodes have Any pins (no per-parameter types in the binding table); no
  per-member pin types beyond the native set.
- No Scratch-like block mode (design note in the guide).
