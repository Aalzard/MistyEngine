# Round 65: a game made only with nodes

A short 2D platformer ("Lumo's Leap") built with Script Graphs only: no Lua, no Rust gameplay
code. The point is to prove the nodes a real game needs, and to list what is broken or missing.

## Issues

Each issue: how to reproduce, the graph, expected and actual. Status is updated when a fix is
merged.

### 1. 2D triggers and collisions never reach Script Graphs

- **Repro:** an object with a Rigidbody 2D, a Box Collider 2D and a graph with
  `On Trigger Enter -> Debug.Log("hit")` (or `On Collision Enter`). Move it into another 2D
  collider (trigger or solid).
- **Graph:** `OnTriggerEnter.Out -> Api(Debug.Log).In`, `OnTriggerEnter.Other -> Message`.
- **Expected:** the event runs with `Other` = the other object, as it does for 3D colliders (and as
  Lua gets `OnTriggerEnter2D` / `OnCollisionEnter2D`).
- **Actual:** nothing runs. `ScriptHost::dispatch_2d`
  (`crates/misty_engine/src/scripting/mod.rs`) calls only Lua instances; the 3D `dispatch` calls
  `self.graphs.contact(...)` first, the 2D one never does. A 2D game cannot use the four contact
  events at all.
- **Workaround used:** distance checks and `Physics2D.OverlapCircleAll` each frame.
- **Status:** reported.

### 2. A UI Button's On Click cannot run a Script Graph

- **Repro:** a Button whose On Click entry targets an object that has a graph with
  `Custom Event: Play`, method `Play`. Click the button while playing.
- **Expected:** the graph's `Custom Event: Play` runs (the entry is "call `method` on the scripts
  of `target`", and graphs are scripts; `gameObject:SendCustomEvent` already reaches both).
- **Actual:** a Console warning, `On Click: no script on 'Menu' has a method 'Play'`, and nothing
  runs. `ScriptHost::send_message_with` only looks at Lua instances; the same holds for On Value
  Changed and On End Edit of the other widgets. There is no node to listen to a button either
  (`button.onClick:AddListener` needs a Lua function).
- **Workaround used:** the menu reads keys and the mouse with `Input` nodes.
- **Status:** reported.

### 3. API nodes of variadic Lua functions have no input pins

- **Repro:** in the node search pick `Physics2D: Raycast`, `Rigidbody2D: AddForce`,
  `Rigidbody2D: MovePosition`, `Input: GetButton`.
- **Expected:** pins for the arguments (origin, direction, distance, layer mask…).
- **Actual:** the node has no inputs: the binding's signature is `(...)`, and
  `signature_args` (`crates/misty_editor/src/panels/script_graph/search.rs`) drops `...`. The
  node can only be used after typing names into Graph Properties ▸ Arguments, which nothing
  hints at.
- **Workaround used:** `args` typed by hand (`origin, direction, distance, mask`).
- **Status:** reported.

### 4. No typed 2D physics nodes (missing nodes)

- **Repro:** look for 2D versions of the native `Physics: Raycast` node (pure, with typed
  `Hit`, `Point`, `Normal`, `Distance`, `Collider` outputs) and of `Rigidbody: Get / Set
  velocity`.
- **Actual:** `Physics2D: Raycast`, `OverlapCircle`, `OverlapCircleAll`, `OverlapBox`,
  `OverlapPoint` and every `Rigidbody2D` member only exist as Lua-binding nodes: all pins are
  Any, the queries are action nodes with exec pins although they only read, and a raycast's
  result is an opaque Lua table (test it with `Not Equal` against nothing, read it with
  `Get Lua Field`).
- **Graph used:** `Physics2D: Raycast (origin, direction, distance, mask) -> Result`,
  `Result -> Not Equal (B empty) -> Branch`.
- **Expected:** native nodes like the 3D ones: `Physics2D: Raycast` pure with `Hit` (Bool),
  `Point`, `Normal`, `Distance`, `Collider` (GameObject); `Rigidbody2D: Get / Set velocity` as
  Vector3; the overlap queries pure, returning GameObject / List.
- **Workaround used:** the Lua-binding nodes above (they work).
- **Status:** reported.

## Other findings (not node bugs)

Found while building the game; listed so they are not lost.

### A. PlayMode tests never end when the game itself loads scenes

- **Repro:** a `*.playtest.lua` that loops `yield(0.5)` over a game which calls
  `SceneManager.LoadScene` every few seconds (the node game restarting a stage when the player
  dies). `misty --run-tests <project> --mode PlayMode` ran at 100 % CPU for 5 minutes, until
  killed.
- **Cause:** a game-side `LoadScene` starts a new session whose `Time.time` restarts at 0
  (`testing/run.rs`, `game_request`). `yield(seconds)` waits for `Time.time >= target` of the
  old clock, and the test timeout compares the session's game time (`run.rs` ~l.548), so
  neither is reached while the game keeps loading scenes. There is no wall-clock limit.
- **Expected:** `yield` and the timeout count game time across scene loads.
- **Workaround:** `yieldFrames(n)`.

### B. A frictionless player still sticks to walls (2D friction is averaged)

- **Repro:** a Rigidbody 2D player whose Box Collider 2D has Friction 0, pushed sideways
  (velocity set each Fixed Update) against the side of a Tilemap Collider 2D or Box Collider 2D
  with the default Friction 0.4, in the air.
- **Actual:** it hangs on the wall and slides down very slowly: the two frictions are averaged
  (0.2), `physics2d/colliders.rs` sets `.friction(...)` only (Rapier's default combine rule).
- **Expected:** like the familiar 2D engines, the combined friction is the geometric mean (or
  the minimum), so one frictionless collider is enough.
- **Workaround used:** Friction 0 on the ground tilemap and the side walls too.

### C. `MISTY_WINDOW_SIZE` capture windows open maximised

- **Repro:** `MISTY_WINDOW_SIZE=1100x650 MISTY_SCREENSHOT=out.png misty <project>` (also with
  `800x500`). The screenshot is 1920x1032 and the window reports `IsZoomed = true`, rect
  `-8,-8,1928,1040`, on a 1920x1080 display.
- **Expected:** a 1100x650 window at the top left, as `main.rs` (~l.443) intends.

### D. The MCP bridge has no Script Graph tools

- `misty --mcp` can create objects, components, Lua scripts and materials, but not `.vgraph`
  files or their nodes and wires, and `scene.add_component` attaches "a Lua script by path".
  The game's graphs were written as files (`tools/node_game`).
