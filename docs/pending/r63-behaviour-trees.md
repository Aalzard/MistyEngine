# r63-behaviour-trees: Behaviour Trees package

Branch `r63-behaviour-trees` from main b88e54c9. Target `E:\MistyTargets\r63-bt`
(`CARGO_INCREMENTAL=0`), TMP `C:\MistyTry\r63tmp`, editor runs under `C:\MistyTry\r63\`.

## Status

See the checklist at the bottom. Commits so far are on the branch; nothing pushed.

## What it is

An **optional package** (Package Manager ▸ Built-in ▸ Behaviour Trees ▸ Install), not
installed by default: a visual AI editor for enemies and NPCs.

### Optional package mechanism

Main's `packages/optional.rs` (from r61-recorder, merged in 97cc0dff): Behaviour Trees is a
second row of its `PACKAGES` table, `BEHAVIOUR_TREES = "com.misty.behaviour-trees"`. Gating
uses `EditorState::optional_packages` like the Recorder: Window ▸ AI ▸ Behaviour Tree,
Assets ▸ Create ▸ Behaviour Tree and Add Archetype ▸ Behaviour Tree Agent appear only while
installed (the Create menu and Add Archetype list have no state, so the window mirrors the flag
into `behaviour_tree::package_installed()` once per frame). The window shows "Install
Behaviour Trees" when not installed. The runtime is always in the engine, so scenes with agents
play (and built games run them) either way.

### Engine (`crates/misty_engine/src/behaviour_tree/`)

- `asset.rs`: `.btree` JSON (nodes with `type` + settings, children in run order, name,
  position; keys; comments; groups), editing helpers (`link` keeps children sorted by height,
  refuses loops; `arrange` lays out from the Root; `rename_key`).
- `blackboard.rs`: typed keys (Bool, Int, Float, Vector, Object, String), per-agent
  `Blackboard` with slot indices and per-slot versions, `SharedBlackboards` by share group.
- `compile.rs`: flat depth-first nodes (parent, children, subtree end), keys resolved to slots,
  observers (conditionals with an abort mode), errors (missing / wrong-type keys, child counts,
  unreachable nodes, Lower Priority without a composite).
- `exec.rs`: the tick. Per-agent `ExecState` sized once (no allocation per tick); observers
  run first and only when their key's version changed (event-driven aborts: Self, Lower
  Priority, Both); then the running path is walked. Deterministic: xorshift seeded per agent
  (seed or FNV of the object's name). `Host` trait = everything that touches the world.
- `component.rs`: Behaviour Tree Agent (tree, tick rate, start on play, restart when done,
  share group, seed, blackboard overrides with object refs remapped by scene files).
- `runtime.rs`: `BtRuntime` in the script host's `World`: agents (picked up / dropped by
  `sync` every frame), compiled trees cache, shared blackboards, `DebugView`.
- `scripting/behaviour_tree_api.rs`: `GameHost` (navigation, physics `line_hit` without
  allocation, animator, Lua, custom events), the per-frame update (after Script Graphs'
  Update, before navigation), the `BehaviourTree` Lua table.

### Editor (`crates/misty_editor/src/panels/behaviour_tree/`)

Window with canvas (node_canvas grid / wires / pan-zoom like Script Graphs), Nodes palette
(drag & drop or click; custom Lua leaves `-- @behaviour-leaf` and Script Graph custom events
discovered in Assets), Node panel, Blackboard panel (keys + live values), toolbar (Save Asset,
undo/redo, Arrange, problems, agent picker in Play). Undo/redo, copy/cut/paste, duplicate,
delete, group (Ctrl+G), comments, linking by dragging an output, rubber band selection.
Live debugging colours (running yellow, success green, failure red, fading).

`panels/inspector/bt_editors.rs`: the agent's Properties.

### Demo

`sample_behaviour_tree.rs` generates `SampleProject/Assets/Scenes/BehaviourTreeDemo.scene`,
its nav mesh, `Assets/AI/Guard.btree`, `NextWaypoint.lua` (custom leaf), `GuardBrain.lua`
(Run Lua `Attack`), `PlayerMove.lua`. Regenerate:
`cargo test -p misty_editor --release -- --ignored write_behaviour_tree_demo`.
SampleProject's manifest does **not** install the package (the scene plays without it).

### Docs

- User Guide chapter `docs/guide/{en,ar}/12-behaviour-trees.md` (+ index entries).
- `docs/api` regenerated for the `BehaviourTree` Lua table.

## Checklist

- [x] Engine module, Lua API, host, tests (semantics table, aborts, blackboard, round trip,
      agent in scene)
- [x] Editor window, palette, inspector, blackboard, agent Properties, gating
- [x] Demo generator + plays test
- [x] Guide chapter EN/AR, locales TR/AR
- [x] Build, fix compile errors; BT tests pass (engine 22, editor 9, demo plays)
- [x] Demo files written; merged main 97cc0dff (Recorder)
- [x] Regenerate docs/api
- [x] Merged main c6159e41 (themes): editor colours use the graph tokens / theme::shade
- [x] fmt clean, clippy 0 warnings, full workspace tests green (editor 1004 passed / 88 ignored, engine 1649 / 111, others green)
- [ ] Editor smoke run on a copy under C:\MistyTry\r63\
